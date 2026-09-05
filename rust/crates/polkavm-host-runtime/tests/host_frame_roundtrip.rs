/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::{BackendKind, PresentationProfile, Runtime};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/host-frame-roundtrip.polkavm");
const REQUEST: &[u8] = b"host-frame-conformance-request-v1";
const RESPONSE: &[u8] = b"host-frame-conformance-response-v1";
const SUCCESS: &[u8] = b"host-frame-roundtrip-ok";

#[test]
fn fixture_imports_the_v2_host_frame_transport() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();
    assert!(imports.iter().any(|symbol| symbol == b"host_frame_send"));
    assert!(imports.iter().any(|symbol| symbol == b"host_frame_poll"));
}

#[test]
fn native_runtime_roundtrips_an_opaque_host_frame() {
    let mut runtime = Runtime::new_with_backend(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Framebuffer,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .expect("create runtime");

    runtime.init().expect("initialize guest");
    assert_eq!(runtime.take_host_frame_request().as_deref(), Some(REQUEST));
    assert!(runtime.take_host_frame_request().is_none());

    runtime
        .send_host_frame_response(RESPONSE.to_vec())
        .expect("queue response");
    runtime.update().expect("deliver response");
    assert_eq!(runtime.take_save().as_deref(), Some(SUCCESS));
    assert!(runtime.take_save().is_none());
}
