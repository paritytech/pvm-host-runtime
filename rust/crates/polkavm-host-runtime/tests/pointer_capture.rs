/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{
    BackendKind, PresentationProfile, Runtime, POINTER_CAPTURE_ACTIVE, POINTER_CAPTURE_ARMED,
    POINTER_CAPTURE_IMPORT, POINTER_CAPTURE_INVALID_REQUEST, POINTER_CAPTURE_RELEASED,
    POINTER_CAPTURE_UNSUPPORTED,
};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/pointer-capture.polkavm");

/// The guest arms, sends an undefined request, releases, then arms again, and
/// saves the four statuses it received.
fn reported_status(runtime: &mut Runtime) -> Vec<i32> {
    let save = runtime.take_save().expect("guest should report its status");
    save.chunks_exact(4)
        .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
        .collect()
}

fn capture_runtime() -> Runtime {
    Runtime::new_with_backend(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .expect("create runtime")
}

#[test]
fn fixture_imports_the_pointer_capture_hostcall() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    assert!(blob
        .imports()
        .iter()
        .flatten()
        .any(|symbol| symbol.as_bytes() == POINTER_CAPTURE_IMPORT.as_bytes()));
}

#[test]
fn a_host_without_capture_support_answers_every_request_alike() {
    let mut runtime = capture_runtime();
    assert!(runtime.uses_pointer_capture());
    runtime.init().expect("guest init should succeed");
    assert_eq!(
        reported_status(&mut runtime),
        vec![POINTER_CAPTURE_UNSUPPORTED; 4]
    );
    assert_eq!(runtime.take_pointer_capture_request(), None);
}

#[test]
fn a_supporting_host_answers_arm_release_and_undefined_requests() {
    let mut runtime = capture_runtime();
    runtime.set_pointer_capture_supported(true);
    runtime.init().expect("guest init should succeed");
    assert_eq!(
        reported_status(&mut runtime),
        vec![
            POINTER_CAPTURE_ARMED,
            POINTER_CAPTURE_INVALID_REQUEST,
            POINTER_CAPTURE_RELEASED,
            POINTER_CAPTURE_ARMED,
        ]
    );
    assert_eq!(runtime.take_pointer_capture_request(), Some(true));
    assert_eq!(runtime.take_pointer_capture_request(), None);
}

#[test]
fn revoking_support_drops_an_unserved_request() {
    let mut runtime = capture_runtime();
    runtime.set_pointer_capture_supported(true);
    runtime.init().expect("guest init should succeed");
    assert_eq!(
        reported_status(&mut runtime).first().copied(),
        Some(POINTER_CAPTURE_ARMED)
    );

    runtime.set_pointer_capture_supported(false);
    assert_eq!(
        runtime.take_pointer_capture_request(),
        None,
        "a Host that cannot capture must not act on an unserved request"
    );
}

#[test]
fn active_capture_is_reported_to_a_guest_that_asks_again() {
    let mut runtime = capture_runtime();
    runtime.set_pointer_capture_supported(true);
    runtime
        .set_pointer_capture_active(true)
        .expect("capture start should reach the guest");
    runtime.init().expect("guest init should succeed");
    let status = reported_status(&mut runtime);
    assert_eq!(status[0], POINTER_CAPTURE_ACTIVE);
    assert_eq!(status[2], POINTER_CAPTURE_ACTIVE);
}
