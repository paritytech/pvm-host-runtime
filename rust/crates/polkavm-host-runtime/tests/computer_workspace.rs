/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerStatus, ComputerSupervisor};

const DRIVER: &[u8] = include_bytes!("fixtures/computer-workspace-driver.polkavm");
const PANE: &[u8] = include_bytes!("fixtures/computer-workspace-pane.polkavm");
const EXTRA: &[u8] = include_bytes!("fixtures/computer-core-services.polkavm");

fn supervisor(program: &[u8]) -> ComputerSupervisor {
    ComputerSupervisor::new_with_backend(
        program,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create supervisor")
}

/// Drives the workspace driver to completion, playing the Host: at the
/// driver's `mount:ready` checkpoint it mounts `/home/seed.txt` (proving
/// live parent->child mount propagation), and it settles open package
/// resolutions through `resolve` (returning `None` rejects as NOT_FOUND).
fn run_driver(
    supervisor: &mut ComputerSupervisor,
    resolve: impl Fn(&str) -> Option<Vec<u8>>,
) -> (i32, String, Vec<String>) {
    let mut output = String::new();
    let mut mounted = false;
    let mut resolutions = Vec::new();
    for _ in 0..10_000 {
        let status = supervisor.run().unwrap();
        if let Some(bytes) = supervisor.take_terminal_output() {
            output.push_str(&String::from_utf8_lossy(&bytes));
        }
        match status {
            ComputerStatus::Exited(code) => return (code, output, resolutions),
            ComputerStatus::Yielded => {
                if !mounted && output.contains("mount:ready") {
                    mounted = true;
                    supervisor
                        .mount_file("/home/seed.txt", b"seed".to_vec())
                        .unwrap();
                    supervisor.send_terminal_input(b"g").unwrap();
                }
            }
            ComputerStatus::PackageRequested => {
                let name = supervisor.pending_package().expect("pending name");
                resolutions.push(name.clone());
                match resolve(&name) {
                    Some(program) => supervisor.provide_package(program).unwrap(),
                    None => supervisor.reject_package(-4).unwrap(),
                }
            }
            status => panic!("unexpected status {status:?}"),
        }
    }
    panic!("driver did not exit");
}

#[test]
fn fixture_imports_versioned_workspace_operations() {
    let blob = ProgramBlob::parse(DRIVER.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();

    for required in [
        b"polkadot_host_0_1_workspace_spawn".as_slice(),
        b"polkadot_host_0_1_workspace_send_input".as_slice(),
        b"polkadot_host_0_1_workspace_read".as_slice(),
        b"polkadot_host_0_1_workspace_resize".as_slice(),
        b"polkadot_host_0_1_workspace_wait".as_slice(),
        b"polkadot_host_0_1_workspace_close".as_slice(),
    ] {
        assert!(
            imports.iter().any(|symbol| symbol == required),
            "missing import {}",
            String::from_utf8_lossy(required)
        );
    }
}

#[test]
fn workspace_guest_supervises_an_independent_child() {
    let mut supervisor = supervisor(DRIVER);
    supervisor.register_package("pane", PANE.to_vec()).unwrap();
    supervisor
        .register_package("extra", EXTRA.to_vec())
        .unwrap();
    supervisor.set_workspace_enabled(true);

    // The driver asserts every contract detail internally (bad handles,
    // unknown package, invalid geometry, banner, byte roundtrip, resize
    // observability, nested denial, persistence, live seed mounts, exit
    // reporting, EOF after drain, close-once) and exits nonzero with a
    // distinct code on the first violation.
    let (code, output, resolutions) = run_driver(&mut supervisor, |_| None);
    assert_eq!(code, 0, "driver output: {output}");
    assert!(output.ends_with("workspace:ok"), "driver output: {output}");
    assert!(resolutions.is_empty());

    // The pane's `/home` write surfaced through the parent supervisor.
    let modified = supervisor.take_modified_files();
    assert!(
        modified
            .iter()
            .any(|(path, bytes)| path == "/home/pane.txt" && bytes == b"from-pane"),
        "pane write should be visible in the shared /home store: {modified:?}"
    );
}

#[test]
fn open_resolution_supplies_packages_anywhere_in_the_tree() {
    // Nothing is pre-registered: the workspace spawn of `pane` and the
    // pane's own foreground run of `extra` both suspend for the embedder,
    // and the driver's unknown-package probe resolves through rejection.
    let mut supervisor = supervisor(DRIVER);
    supervisor.set_workspace_enabled(true);
    supervisor.set_package_resolution(true);

    let (code, output, resolutions) = run_driver(&mut supervisor, |name| match name {
        "pane" => Some(PANE.to_vec()),
        "extra" => Some(EXTRA.to_vec()),
        _ => None,
    });
    assert_eq!(code, 0, "driver output: {output}");
    assert!(output.ends_with("workspace:ok"), "driver output: {output}");
    assert_eq!(
        resolutions,
        vec![
            "no-such-package".to_owned(),
            "pane".to_owned(),
            "extra".to_owned()
        ]
    );
}

#[test]
fn workspace_operations_are_denied_without_the_grant() {
    let mut supervisor = supervisor(DRIVER);
    supervisor.register_package("pane", PANE.to_vec()).unwrap();
    supervisor
        .register_package("extra", EXTRA.to_vec())
        .unwrap();

    // Without set_workspace_enabled the driver's first probe observes
    // DENIED and exits with its distinct gating code.
    let (code, _, _) = run_driver(&mut supervisor, |_| None);
    assert_eq!(code, 41);
}
