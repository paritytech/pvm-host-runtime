/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]
#![allow(static_mut_refs)]

const REQUEST: &[u8] = b"host-frame-conformance-request-v1";
const RESPONSE: &[u8] = b"host-frame-conformance-response-v1";
const SUCCESS: &[u8] = b"host-frame-roundtrip-ok";
const RESPONSE_CAPACITY: usize = 64;

static mut RESPONSE_BUFFER: [u8; RESPONSE_CAPACITY] = [0; RESPONSE_CAPACITY];
static mut COMPLETE: bool = false;

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_frame_send(pointer: u32, length: u32) -> u32;
    fn host_frame_poll(pointer: u32, capacity: u32) -> i32;
    fn host_save_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    let status = unsafe { host_frame_send(REQUEST.as_ptr() as u32, REQUEST.len() as u32) };
    assert_eq!(status, 0);
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    if unsafe { COMPLETE } {
        return;
    }

    let length = unsafe {
        host_frame_poll(
            RESPONSE_BUFFER.as_mut_ptr() as u32,
            RESPONSE_BUFFER.len() as u32,
        )
    };
    if length == 0 {
        return;
    }
    assert!(length > 0);
    assert_eq!(length as usize, RESPONSE.len());
    assert_eq!(unsafe { &RESPONSE_BUFFER[..length as usize] }, RESPONSE);

    let status = unsafe { host_save_submit(SUCCESS.as_ptr() as u32, SUCCESS.len() as u32) };
    assert_eq!(status, 0);
    unsafe { COMPLETE = true };
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
