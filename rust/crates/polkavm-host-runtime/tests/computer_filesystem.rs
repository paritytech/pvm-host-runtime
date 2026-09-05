/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerStatus, ComputerSupervisor};

const PROGRAM: &[u8] = include_bytes!("fixtures/computer-filesystem.polkavm");

fn launch(backend: BackendKind, args: Vec<String>) -> ComputerSupervisor {
    let mut supervisor = ComputerSupervisor::new_with_backend(
        PROGRAM,
        ComputerContext::new(args, vec![]).unwrap(),
        50_000_000,
        backend,
    )
    .unwrap();
    supervisor
        .register_package("fs-child", PROGRAM.to_vec())
        .unwrap();
    supervisor
}

fn checkpoint(supervisor: &mut ComputerSupervisor, expected: &str) {
    let mut output = String::new();
    for _ in 0..100 {
        let status = supervisor.run().unwrap();
        while let Some(bytes) = supervisor.take_terminal_output() {
            output.push_str(&String::from_utf8_lossy(&bytes));
        }
        if output.contains(expected) {
            return;
        }
        assert_eq!(status, ComputerStatus::Yielded, "{output}");
    }
    panic!("missing checkpoint {expected}: {output}");
}

#[test]
fn guest_shared_lock_metadata_directory_records_and_atomic_publication() {
    for backend in [BackendKind::Interpreter, BackendKind::Compiler] {
        let mut supervisor = launch(backend, vec![]);
        checkpoint(&mut supervisor, "fs:ready");
        supervisor.send_terminal_input(b"p").unwrap();
        checkpoint(&mut supervisor, "fs:published");
        assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(0));
        assert_eq!(
            supervisor.take_modified_files(),
            vec![("/home/repo/record".into(), b"candidate".to_vec())]
        );
    }
}

#[test]
fn cancellation_before_publication_preserves_destination_and_restores_empty_directories() {
    for backend in [BackendKind::Interpreter, BackendKind::Compiler] {
        let mut supervisor = launch(backend, vec![]);
        checkpoint(&mut supervisor, "fs:ready");
        supervisor.send_terminal_input(b"c").unwrap();
        checkpoint(&mut supervisor, "fs:cancel");
        assert_eq!(
            supervisor.terminate_foreground().unwrap(),
            ComputerStatus::Exited(130)
        );
        let metadata = supervisor.export_filesystem_metadata();
        let files = supervisor.take_modified_files();
        assert!(files
            .iter()
            .any(|(path, bytes)| path == "/home/repo/record" && bytes == b"new"));
        let mut restored = launch(backend, vec!["check".into()]);
        for (path, bytes) in files {
            restored.mount_file(&path, bytes).unwrap();
        }
        restored
            .import_filesystem_metadata(metadata.clone())
            .unwrap();
        assert_eq!(restored.export_filesystem_metadata(), metadata);
        checkpoint(&mut restored, "fs:restored");
        assert_eq!(restored.run().unwrap(), ComputerStatus::Exited(0));
    }
}
