/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Reports what `host_pointer_capture` answers for an arm, an undefined
//! request, a release, and a second arm. The Host reads the four little-endian
//! `i32` statuses back as one save payload, so the same guest verifies a Host
//! with capture support and a Host without one.

#![no_std]

const ARM: u32 = 1;
const RELEASE: u32 = 0;
const UNDEFINED: u32 = 7;

static mut STATUS: [i32; 4] = [0; 4];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_pointer_capture(request: u32) -> i32;
    fn host_save_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    unsafe {
        STATUS[0] = host_pointer_capture(ARM);
        STATUS[1] = host_pointer_capture(UNDEFINED);
        STATUS[2] = host_pointer_capture(RELEASE);
        STATUS[3] = host_pointer_capture(ARM);
        submit_status();
    }
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    unsafe { submit_status() }
}

unsafe fn submit_status() {
    let pointer = core::ptr::addr_of!(STATUS) as u32;
    let length = core::mem::size_of::<[i32; 4]>() as u32;
    host_save_submit(pointer, length);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
