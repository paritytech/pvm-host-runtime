# Host-frame round-trip conformance guest

This guest proves the PolkaVM application ABI v2 host-frame transport without
interpreting the opaque frame bytes.

Expected sequence:

1. `init` submits `host-frame-conformance-request-v1` through `host_frame_send`.
2. The Host takes that request and supplies
   `host-frame-conformance-response-v1` as the next response.
3. A later `update` receives the response through `host_frame_poll` and
   submits `host-frame-roundtrip-ok` as save data.

The checked `.polkavm` fixture is built with the repository's pinned guest
build inputs and consumed unchanged by native and browser conformance tests.
