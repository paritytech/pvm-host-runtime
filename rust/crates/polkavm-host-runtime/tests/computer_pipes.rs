/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerStatus, ComputerSupervisor};

const DRIVER: &[u8] = include_bytes!("fixtures/computer-pipe-driver.polkavm");
const FILTER: &[u8] = include_bytes!("fixtures/computer-pipe-filter.polkavm");

#[test]
fn fixture_imports_versioned_process_pipe_operations() {
    let blob = ProgramBlob::parse(DRIVER.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();

    for required in [
        b"polkadot_host_0_1_process_spawn".as_slice(),
        b"polkadot_host_0_1_process_wait".as_slice(),
        b"polkadot_host_0_1_pipe_read".as_slice(),
        b"polkadot_host_0_1_pipe_write".as_slice(),
        b"polkadot_host_0_1_pipe_close".as_slice(),
    ] {
        assert!(
            imports.iter().any(|symbol| symbol == required),
            "missing import {}",
            String::from_utf8_lossy(required)
        );
    }
}

#[test]
fn guest_streams_bytes_through_a_piped_child_and_reaps_it() {
    let mut supervisor = ComputerSupervisor::new_with_backend(
        DRIVER,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create supervisor");
    supervisor
        .register_package("upper", FILTER.to_vec())
        .unwrap();

    // The driver asserts every contract detail internally (unknown package,
    // bad pids, partial writes, EOF, double reap) and exits nonzero with a
    // distinct code on the first violation.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(0));
    assert_eq!(
        supervisor.take_terminal_output().as_deref(),
        Some(b"HELLO, PIPES".as_slice())
    );
}

#[test]
fn spawn_without_registration_fails_from_the_start() {
    let mut supervisor = ComputerSupervisor::new_with_backend(
        DRIVER,
        ComputerContext::default(),
        50_000_000,
        BackendKind::Interpreter,
    )
    .expect("create supervisor");

    // Without the `upper` package the driver's spawn fails; it exits with
    // its distinct spawn-failure code rather than success.
    assert_eq!(supervisor.run().unwrap(), ComputerStatus::Exited(13));
}
