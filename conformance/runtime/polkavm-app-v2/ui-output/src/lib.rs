/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#![no_std]

const INVALID: [u8; 48] = [0; 48];
const OUTPUT: [u8; 89] = [
    80, 85, 73, 49, 1, 0, 48, 0, 89, 0, 0, 0, 2, 0, 9, 3, 0, 0, 32, 65, 0, 0, 160, 65, 0, 0, 82,
    67, 0, 0, 112, 66, 0, 0, 192, 65, 0, 0, 176, 65, 0, 0, 200, 65, 0, 0, 104, 66, 1, 0, 0, 0, 5,
    0, 0, 0, 104, 101, 108, 108, 111, 2, 1, 0, 0, 20, 0, 0, 0, 104, 116, 116, 112, 115, 58, 47, 47,
    101, 120, 97, 109, 112, 108, 101, 46, 116, 101, 115, 116,
];

#[polkavm_derive::polkavm_import]
extern "C" {
    fn host_ui_output_submit(pointer: u32, length: u32) -> u32;
}

#[polkavm_derive::polkavm_export]
extern "C" fn init() {
    let status = unsafe { host_ui_output_submit(INVALID.as_ptr() as u32, INVALID.len() as u32) };
    assert_eq!(status, 1);
    let invalid_range = unsafe { host_ui_output_submit(u32::MAX - 7, INVALID.len() as u32) };
    assert_eq!(invalid_range, 1);
}

#[polkavm_derive::polkavm_export]
extern "C" fn update() {
    let accepted = unsafe { host_ui_output_submit(OUTPUT.as_ptr() as u32, OUTPUT.len() as u32) };
    assert_eq!(accepted, 0);
    let duplicate = unsafe { host_ui_output_submit(OUTPUT.as_ptr() as u32, OUTPUT.len() as u32) };
    assert_eq!(duplicate, 2);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
