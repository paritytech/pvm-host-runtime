/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerRuntime, ComputerStatus};

const PROGRAM: &[u8] = include_bytes!("fixtures/computer-core-context.polkavm");
const CORE_SERVICES: &[u8] = include_bytes!("fixtures/computer-core-services.polkavm");
const ROUNDTRIP: &[u8] = include_bytes!("fixtures/computer-tty-fs-roundtrip.polkavm");

#[test]
fn fixture_imports_versioned_computer_core_operations() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();

    assert!(imports
        .iter()
        .any(|symbol| symbol == b"polkadot_host_0_1_core_args"));
    assert!(imports
        .iter()
        .any(|symbol| symbol == b"polkadot_host_0_1_core_environment"));
    assert!(imports
        .iter()
        .any(|symbol| symbol == b"polkadot_host_0_1_core_exit"));
}

#[test]
fn computer_guest_reads_context_and_exits_with_status() {
    let context = ComputerContext::new(
        vec!["shell.polkavm".into(), "--login".into()],
        vec![
            ("HOME".into(), "/home".into()),
            ("TERM".into(), "pvm-tty".into()),
        ],
    )
    .unwrap();
    let mut runtime =
        ComputerRuntime::new_with_backend(PROGRAM, context, 10_000_000, BackendKind::Interpreter)
            .expect("create computer runtime");

    assert_eq!(runtime.run().unwrap(), ComputerStatus::Exited(23));
    assert_eq!(runtime.run().unwrap(), ComputerStatus::Exited(23));
}

#[test]
fn computer_guest_reads_clocks_and_secure_random() {
    let mut runtime = ComputerRuntime::new_with_backend(
        CORE_SERVICES,
        ComputerContext::new(vec![], vec![]).unwrap(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create computer runtime");

    // The fixture verifies monotonic ordering, a plausible wall clock,
    // distinct secure-random outputs, and INVALID/LIMIT boundaries.
    assert_eq!(runtime.run().unwrap(), ComputerStatus::Exited(31));
}

fn drain_output(runtime: &mut ComputerRuntime) -> Vec<u8> {
    let mut output = Vec::new();
    while let Some(bytes) = runtime.take_terminal_output() {
        output.extend_from_slice(&bytes);
    }
    output
}

#[test]
fn computer_guest_roundtrips_terminal_and_filesystem() {
    let mut runtime = ComputerRuntime::new_with_backend(
        ROUNDTRIP,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create roundtrip runtime");
    runtime.set_terminal_size(100, 40).unwrap();
    runtime
        .mount_file("/home/seed.txt", b"seeded".to_vec())
        .unwrap();

    assert_eq!(runtime.run().unwrap(), ComputerStatus::Yielded);
    assert_eq!(drain_output(&mut runtime), b"ready:seeded\r\n");
    assert_eq!(runtime.terminal_mode(), polkavm_host_runtime::TTY_MODE_RAW);
    assert!(runtime.take_modified_files().is_empty());
    assert!(runtime.take_removed_files().is_empty());

    runtime.send_terminal_input(b"hello").unwrap();
    assert_eq!(runtime.run().unwrap(), ComputerStatus::Yielded);
    assert_eq!(drain_output(&mut runtime), b"HELLO");

    runtime.send_terminal_input(b" pvm").unwrap();
    assert_eq!(runtime.run().unwrap(), ComputerStatus::Yielded);
    assert_eq!(drain_output(&mut runtime), b" PVM");

    runtime.send_terminal_input(b"q").unwrap();
    assert_eq!(runtime.run().unwrap(), ComputerStatus::Exited(7));
    assert_eq!(runtime.exit_status(), Some(7));
    assert_eq!(
        runtime.take_modified_files(),
        vec![("/home/echo.txt".to_owned(), b"hello pvm".to_vec())]
    );
    assert_eq!(
        runtime.take_removed_files(),
        vec!["/home/remove.tmp".to_owned()]
    );

    let mut relaunched = ComputerRuntime::new_with_backend(
        ROUNDTRIP,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create relaunched runtime");
    relaunched
        .mount_file("/home/seed.txt", b"hello pvm".to_vec())
        .unwrap();
    assert_eq!(relaunched.run().unwrap(), ComputerStatus::Yielded);
    assert_eq!(drain_output(&mut relaunched), b"ready:hello pvm\r\n");
}

#[test]
fn supervisor_terminates_root_as_interrupted() {
    let mut supervisor = polkavm_host_runtime::ComputerSupervisor::new_with_backend(
        ROUNDTRIP,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create supervisor");
    supervisor.set_terminal_size(100, 40).unwrap();
    supervisor
        .mount_file("/home/seed.txt", b"seeded".to_vec())
        .unwrap();

    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);
    supervisor.send_terminal_input(b"hello").unwrap();
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Yielded);

    // Host-authority cancellation of the root ends the computer with the
    // interrupted status; the guest never reached its `q` save path, so no
    // phantom writes surface.
    assert_eq!(
        supervisor.terminate_foreground().unwrap(),
        ComputerStatus::Exited(130)
    );
    assert!(supervisor.take_modified_files().is_empty());
    // Termination is recorded: the computer stays exited on later runs.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(130));
}
