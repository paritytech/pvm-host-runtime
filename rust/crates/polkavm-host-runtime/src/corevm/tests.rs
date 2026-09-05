/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use super::*;
use polkavm_common::program::{asm, InstructionSetKind};
use polkavm_common::writer::ProgramBlobBuilder;

#[test]
fn corevm_accepts_the_motion_hostcall() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(b"host_motion_read");
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ecalli(0), asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert_eq!(vm.import_motion_read, Some(0));
    assert!(vm.uses_motion());
}

#[test]
fn corevm_reports_when_motion_is_not_imported() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert!(!vm.uses_motion());
}

#[test]
fn corevm_accepts_and_reports_the_pointer_capture_hostcall() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(crate::POINTER_CAPTURE_IMPORT.as_bytes());
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let mut vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    assert!(vm.uses_pointer_capture());

    vm.set_pointer_capture_supported(true);
    assert_eq!(
        vm.pointer_capture.request(crate::POINTER_CAPTURE_ARM),
        crate::POINTER_CAPTURE_ARMED
    );
    assert_eq!(vm.take_pointer_capture_request(), Some(true));
}

#[test]
fn corevm_capture_state_reaches_the_extended_input_queue() {
    let mut builder = ProgramBlobBuilder::new(InstructionSetKind::Latest32);
    builder.set_stack_size(4 * 1024);
    builder.add_import(crate::POINTER_CAPTURE_IMPORT.as_bytes());
    builder.add_export_by_basic_block(0, b"_pvm_start");
    builder.set_code(&[asm::ret()], &[]);
    let blob = ProgramBlob::parse(builder.into_vec().unwrap().into()).unwrap();
    let mut vm = Vm::from_blob(blob, polkavm::BackendKind::Interpreter).unwrap();
    vm.set_pointer_capture_supported(true);
    vm.set_pointer_capture_active(true).unwrap();
    assert_eq!(
        vm.epoca_input_events.pop_front(),
        Some(crate::ui::pointer_capture_record(true))
    );
}

#[test]
fn open_files_enforce_the_descriptor_limit() {
    let file = Arc::new(File { blob: Vec::new() });
    let mut open_files = OpenFiles::new();
    for expected in 3..3 + MAX_OPEN_FILES as u64 {
        assert_eq!(open_files.open(Arc::clone(&file)), Ok(expected));
    }
    assert_eq!(open_files.open(Arc::clone(&file)), Err(EMFILE));
    assert_eq!(open_files.descriptors.len(), MAX_OPEN_FILES);

    assert!(open_files.remove(3).is_some());
    assert_eq!(
        open_files.open(file),
        Ok(3 + MAX_OPEN_FILES as u64),
        "closing a descriptor should restore capacity"
    );
}

#[test]
fn seeks_reject_negative_offsets_without_clamping_valid_offsets() {
    assert_eq!(seek_position(5, 10, -6, SEEK_CUR), Err(EINVAL));
    assert_eq!(seek_position(0, 10, -11, SEEK_END), Err(EINVAL));
    assert_eq!(seek_position(0, 10, 12, SEEK_SET), Ok(12));
    assert_eq!(seek_position(5, 10, 3, SEEK_CUR), Ok(8));
    assert_eq!(seek_position(0, 10, -3, SEEK_END), Ok(7));
}

#[test]
fn wrapped_input_chunks_preserve_order_and_destination() {
    let mut events = VecDeque::with_capacity(4);
    for key in 1..=4 {
        events.push_back(InputEvent { key, value: 1 });
    }
    events.pop_front();
    events.pop_front();
    events.push_back(InputEvent { key: 5, value: 1 });
    events.push_back(InputEvent { key: 6, value: 1 });

    let (first, second) = queued_input_chunks(&events, events.len());
    assert!(
        !second.is_empty(),
        "test queue should cross its ring boundary"
    );
    let keys: Vec<_> = first.iter().chain(second).map(|event| event.key).collect();
    assert_eq!(keys, [3, 4, 5, 6]);
    assert_eq!(
        input_destination(100, first.len()).unwrap(),
        100 + u32::try_from(core::mem::size_of_val(first)).unwrap()
    );

    let written = first.len() + second.len();
    for _ in 0..written {
        events.pop_front();
    }
    assert!(events.is_empty(), "every reported event should be consumed");
}

#[test]
fn legacy_mouse_backlog_keeps_only_the_latest_delta() {
    let mut events = VecDeque::new();
    queue_input_event(&mut events, crate::quake_keys::MOUSE_X, 100);
    queue_input_event(&mut events, crate::quake_keys::MOUSE_X, 80);

    assert_eq!(events.len(), 1);
    assert_eq!(events.front().unwrap().value, 80);
}

#[test]
fn epoca_mouse_backlog_keeps_only_the_latest_frame_delta() {
    let mut events = VecDeque::new();
    let key = crate::InputEvent {
        event_type: crate::InputEventType::KeyDown,
        code: 4,
        x: 0,
        y: 0,
    };
    let stale = crate::InputEvent {
        event_type: crate::InputEventType::PointerDelta,
        code: 0,
        x: 100,
        y: (-60_i16) as u16,
    };
    let latest = crate::InputEvent {
        event_type: crate::InputEventType::PointerDelta,
        code: 0,
        x: 12,
        y: (-7_i16) as u16,
    };

    queue_epoca_input_event(&mut events, key);
    queue_epoca_input_event(&mut events, stale);
    queue_epoca_input_event(&mut events, latest);

    assert_eq!(events.len(), 2);
    assert_eq!(events.pop_front(), Some(key.encode()));
    assert_eq!(events.pop_front(), Some(latest.encode()));
}
