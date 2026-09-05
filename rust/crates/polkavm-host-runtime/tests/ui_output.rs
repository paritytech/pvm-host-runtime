/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use polkavm::ProgramBlob;
use polkavm_host_runtime::ui_wire::{
    decode_ui_output, UiCursorIcon, UiOutputCommand, UI_OUTPUT_SUBMIT_IMPORT,
};
use polkavm_host_runtime::{BackendKind, PresentationProfile, Runtime};
use std::collections::HashMap;

const PROGRAM: &[u8] = include_bytes!("fixtures/ui-output.polkavm");

#[test]
fn fixture_imports_the_v2_ui_output_transport() {
    let blob = ProgramBlob::parse(PROGRAM.into()).expect("fixture should be valid PolkaVM");
    let imports: Vec<_> = blob
        .imports()
        .iter()
        .flatten()
        .map(|symbol| symbol.as_bytes().to_vec())
        .collect();
    assert!(imports
        .iter()
        .any(|symbol| symbol == UI_OUTPUT_SUBMIT_IMPORT.as_bytes()));
}

#[test]
fn runtime_rejects_invalid_output_and_retains_one_valid_snapshot_per_call() {
    let mut runtime = Runtime::new_with_backend(
        PROGRAM,
        HashMap::new(),
        PresentationProfile::Tri2d,
        false,
        10_000_000,
        BackendKind::Interpreter,
    )
    .expect("create runtime");

    runtime
        .init()
        .expect("invalid init output should return a status");
    assert!(runtime.take_ui_output().is_none());

    runtime
        .update()
        .expect("valid update output should be accepted");
    let frame = runtime
        .take_ui_output()
        .expect("UI output should be queued");
    let output = decode_ui_output(&frame.bytes).expect("runtime output should remain valid");
    let descriptor = output.descriptor();
    assert_eq!(descriptor.cursor_icon, UiCursorIcon::Text);
    assert!(descriptor.mutable_text_under_cursor);
    assert!(descriptor.ime.is_some());
    assert_eq!(
        output.commands().collect::<Vec<_>>(),
        vec![
            UiOutputCommand::CopyText("hello"),
            UiOutputCommand::OpenUrl {
                url: "https://example.test",
                new_surface: true,
            },
        ]
    );
    assert!(runtime.take_ui_output().is_none());

    runtime
        .update()
        .expect("submission quota should reset each update");
    assert!(runtime.take_ui_output().is_some());
}
