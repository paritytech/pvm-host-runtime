/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::io::{Read, Write};
use std::net::TcpListener;

use polkavm_host_runtime::{BackendKind, ComputerContext, ComputerRuntime, ComputerStatus};

const PROGRAM: &[u8] = include_bytes!("fixtures/computer-tcp-roundtrip.polkavm");

/// Serves exactly one connection: uppercases whatever arrives first.
fn spawn_upper_server() -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept guest connection");
        let mut buffer = [0u8; 64];
        let count = stream.read(&mut buffer).expect("read guest request");
        for byte in &mut buffer[..count] {
            *byte = byte.to_ascii_uppercase();
        }
        stream.write_all(&buffer[..count]).expect("write reply");
    });
    (address, handle)
}

fn run_to_exit(runtime: &mut ComputerRuntime) -> i32 {
    for _ in 0..10_000 {
        match runtime.run().expect("run tcp guest") {
            ComputerStatus::Exited(status) => return status,
            ComputerStatus::Yielded => {}
            other => panic!("unexpected status {other:?}"),
        }
    }
    panic!("guest did not exit");
}

#[test]
fn guest_roundtrips_bytes_over_tcp_when_granted() {
    let (address, server) = spawn_upper_server();
    let context =
        ComputerContext::new(Vec::new(), vec![("NET_TARGET".into(), address.to_string())]).unwrap();
    let mut runtime =
        ComputerRuntime::new_with_backend(PROGRAM, context, 50_000_000, BackendKind::Interpreter)
            .expect("create tcp runtime");
    runtime.set_network_enabled(true);

    assert_eq!(run_to_exit(&mut runtime), 0);
    server.join().unwrap();
}

#[test]
fn network_capability_is_denied_by_default() {
    let context = ComputerContext::new(
        Vec::new(),
        vec![("NET_TARGET".into(), "127.0.0.1:1".into())],
    )
    .unwrap();
    let mut runtime =
        ComputerRuntime::new_with_backend(PROGRAM, context, 50_000_000, BackendKind::Interpreter)
            .expect("create tcp runtime");

    // The guest maps a DENIED connect to its distinct exit code 21.
    assert_eq!(run_to_exit(&mut runtime), 21);
}
