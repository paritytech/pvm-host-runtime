---
title: "PolkaVM application runtime ABI v1"
type: runtime-contract
status: draft
---

# PolkaVM application runtime ABI v1

## Scope

This document defines the application-visible boundary selected by an App v2
manifest with:

```json
{
  "runtime": {
    "kind": "polkavm",
    "abiVersion": 1
  }
}
```

It covers cooperative PolkaVM applications with `init` and `update` exports,
the Host imports available to those applications, guest-memory rules, common
resource bounds, and failure behavior.

Graphics command payloads are defined by the separately versioned Framebuffer,
[Tri2D](tri2d-v1.md), WebGPU Raster, and WebGPU profile contracts. The `_pvm_start` CoreVM
compatibility path is outside this ABI and must be specified separately before
it is advertised as a portable Product runtime.

## Conformance language

The key words MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT, and MAY are to be
interpreted as described in RFC 2119.

A Host advertises PolkaVM application ABI v1 only when its observable behavior
conforms to this document and the conformance fixtures associated with it.

## Program model

The executable is a valid PolkaVM program selected by the App manifest's
archive-relative `runtime.entrypoint`.

The program MUST export:

```text
init() -> ()
update() -> ()
```

The Host instantiates a fresh program, calls `init` exactly once, and calls
`update` zero or more times while the App is running. Calls are serialized; the
Host MUST NOT enter the same program concurrently.

The Host selects and enforces a nonzero gas budget for each call. A trap, gas
exhaustion, invalid guest-memory access, or Host-call budget failure fails the
current execution. ABI v1 does not restart a failed program transparently.

The Host owns scheduling and presentation. Returning from `update` yields
control to the Host; it does not imply that a frame was presented.

## Byte order and guest memory

All integer and sample encodings defined by this ABI are little-endian.
Pointers are `u32` offsets into guest memory. A pointer is valid only for the
duration of the Host call receiving it. The Host MUST copy or consume the
referenced bytes before returning and MUST NOT retain guest pointers.

A Host MUST bounds-check every guest read and write. Integer overflow while
computing a range is an invalid guest-memory access. A failed memory access
fails the current execution unless an individual Host call explicitly defines
a returned status for that condition.

## Capability gating

The App manifest selects exactly one graphics profile and may enable device
input and audio. A Host call made outside its declared capability MUST fail
with that call's unavailable or invalid-state result. The Host MUST NOT
silently reinterpret a submission as another graphics profile.

## Host imports

### Framebuffer presentation

```text
host_present_frame(
  pointer: u32,
  width: u32,
  height: u32,
  stride: u32
) -> u32
```

The call submits one complete packed framebuffer. `stride` MUST equal
`width * 4`. The selected graphics profile MUST be `framebuffer`.

Return values:

```text
0  accepted
1  invalid dimensions, stride, or byte length
3  framebuffer profile unavailable for this execution
```

The Framebuffer profile contract defines pixel order, dimensions, and
presentation semantics.

### Tri2D presentation

```text
host_tri2d_submit(pointer: u32, length: u32) -> u32
```

The call submits one complete Tri2D command stream. The selected graphics
profile MUST be `tri2d`. ABI v1 accepts at most one Tri2D submission during one
`init` or `update` call.

Return values:

```text
0  accepted
1  malformed or out-of-bounds command stream
2  a Tri2D stream was already submitted during this call
3  Tri2D profile unavailable for this execution
```

The [Tri2D profile contract](tri2d-v1.md) defines the command stream and
retained-resource semantics.

### WebGPU capabilities

```text
host_gpu_capabilities(pointer: u32, capacity: u32) -> i32
```

The selected graphics profile MUST be `webgpu-raster` or `webgpu`. The Host
writes the current WebGPU capability record when the supplied capacity is
sufficient. `webgpu-raster` exposes only raster commands. `webgpu` exposes the
same resource table plus compute pipeline and dispatch commands.

Return values:

```text
> 0  capability-record bytes written
  0  capabilities are not ready
< 0  required capacity, represented as the negated byte count, or a stable
     GPU error defined by the selected WebGPU contract
```

### WebGPU submission

```text
host_gpu_submit(pointer: u32, length: u32) -> i32
```

The call submits one complete WebGPU batch. Acceptance means that the batch
passed synchronous Host validation and was queued; it does not imply shader
compilation or GPU completion. A `webgpu-raster` Host MUST reject compute
commands. A `webgpu` Host accepts both raster and compute commands.

Return values are defined by the selected WebGPU contract. ABI v1 reserves:

```text
 0  accepted
 1  bounded backpressure; the guest may retry
-1  invalid guest range
-2  malformed batch
-3  quota exceeded
-4  invalid or stale resource handle
-5  invalid lifecycle or profile state
-6  stopped execution
```

### WebGPU events

```text
host_gpu_receive(pointer: u32, capacity: u32) -> i32
```

The call reads the oldest queued WebGPU event.

```text
> 0  event bytes written
  0  no event is available
< 0  required capacity, represented as the negated byte count, or a stable
     GPU error defined by the selected WebGPU contract
```

### TrUAPI transport

Every ABI v1 application receives a bounded transport for canonical TrUAPI
request and response frames. The runtime treats frame bytes as opaque; TrUAPI
defines their encoding and service semantics.

```text
host_truapi_send(pointer: u32, length: u32) -> u32
```

The call copies one complete request frame into the Host's FIFO request queue.
It returns:

```text
0  accepted
1  empty or larger than the frame limit
2  request queue count or byte limit reached
```

```text
host_truapi_poll(pointer: u32, capacity: u32) -> i32
```

The call reads the oldest complete response frame. A successful read removes
that response from the queue.

```text
> 0  response bytes written
  0  no response is available
< 0  required capacity, represented as the negated byte count; the response
     remains queued
```

Request and response queues are independent. ABI v1 allows frames up to
1 MiB, at most 32 queued frames, and at most 4 MiB of queued frame bytes in
each direction. The Host MUST reject an empty or over-limit response before it
becomes visible to the guest.

TrUAPI transport is part of the base application ABI and does not require a
manifest capability. Product identity, execution kind, permissions, and
service availability remain Host and TrUAPI policy.

### Input

```text
host_poll_input(pointer: u32, capacity: u32) -> u32
```

The Host writes as many complete eight-byte input records as fit in `capacity`
and returns the number of bytes written. It never writes a partial record.
Zero means that no event was available or that the capacity was smaller than
one record.

The legacy fixed record layout is:

```text
offset  type  field
0       u8    event type
1       u8    code
2       u16   x
4       u16   y
6       u16   zero
```

ABI v1 event types are:

```text
1   key down
2   key up
3   pointer button down
4   pointer button up
5   pointer position
6   pointer delta
7   surface metrics
8   committed UTF-8 text chunk
9   IME preedit UTF-8 chunk
10  IME commit UTF-8 chunk
11  IME enabled
12  IME disabled or cancelled
13  focus (`code` is 0 or 1)
14  wheel delta (`x` and `y` are signed i16)
15  pointer capture (`code` is 0 or 1)
```

Pointer button, position, and delta records are baseline optional input. An App
does not list `pointer` in `deviceInput.requiredFeatures`: a Host with no
pointer source simply emits no pointer records, and that absence is not a
launch failure. Pointer capture is Host policy and is never selected by the
manifest. The guest arms capture through the pointer-capture hostcall below,
and the Host decides when an activation is eligible.

Text and IME records use `code` bits 0–2 as a payload length from zero through
six, bit 6 for the first chunk, and bit 7 for the last chunk. Bytes 2–7 contain
the chunk and zero padding. A complete text event is at most 4 KiB. The Host
MUST queue all chunks of one event atomically; the guest MUST reject malformed
flag sequences or invalid UTF-8.

### Pointer capture

```text
host_pointer_capture(request: u32) -> i32
```

The hostcall is part of the base ABI and MUST always resolve. `request` is 1 to
arm capture for the next eligible primary activation and 0 to release capture
and disarm it. A Host without capture support returns `-1` for every value; a
Host with capture support returns `-2` for every other value.

The call returns the resulting policy state:

```text
 0  released: capture is neither armed nor active
 1  armed: capture starts at the next eligible primary activation
 2  active: the Host currently captures the pointer
-1  unsupported: this Host has no pointer-capture policy
-2  invalid request
```

Arming is a request, never a guarantee: the Host owns activation eligibility,
platform permission, and the escape affordance. A Host MUST emit a pointer
capture record for every transition it makes, including capture the user ended,
so a guest that arms capture learns when capture actually started and stopped.
A Host that stops supporting capture MUST report the release first, so `-1` is
never returned while the guest still believes capture is active, and it MUST
discard the arming request it has not served yet.

A request answered with `-1` is not remembered. A Host MAY gain capture support
after the guest initialised, so a guest that still wants capture MUST re-issue
the request on a later update rather than arming once during `init`.

While capture is active the Host delivers pointer delta records; the guest
releases capture whenever it shows a cursor-driven surface such as a menu.

This hostcall is provisional. It is implemented by the reference runtime and
the reference Hosts so that first-party applications can replace Host-guessed
capture, and it is a candidate for the RFC that standardises this ABI. Until
that RFC is accepted, the import name, request values, return codes, and record
type MAY change with this draft, and no third-party Host is expected to
advertise it as a stable contract.

### UI semantics

```text
host_ui_semantics_submit(pointer: u32, length: u32) -> u32
```

The guest may submit one complete UTF-8 JSON semantic tree per `init` or
`update` call. The tree is presentation output, not an instruction to invoke
guest functions. Hosts use its roles, labels, values, actions, focus, and
surface-relative bounds for accessibility and UI automation, then deliver
actual pointer, keyboard, text, or IME records for every interaction.

The version 1 object contains `version`, monotonic `generation`, and `nodes`.
Each node contains a nonzero numeric `id`, nullable `parent`, `role`, `name`,
`value`, `[x0,y0,x1,y1]` bounds, `actions`, `disabled`, and `focused`. Version 1
allows at most 1,024 nodes, 1 KiB per name or value, and 256 KiB for the whole
tree. It requires exactly one root, unique IDs, existing parents, finite ordered
bounds, and no unknown object fields.

Return values:

```text
0  accepted
1  malformed, out-of-bounds, or over-limit tree
2  a tree was already submitted during this call
```

### UI platform output

```text
host_ui_output_submit(pointer: u32, length: u32) -> u32
```

The guest may submit one complete `PUI1` stream per `init` or `update` call.
The stream combines persistent integration state (cursor and text-editor
geometry) with ordered ephemeral commands. It is part of the base ABI and does
not require a manifest capability. Acceptance means that the stream was
validated and queued; it does not imply that platform policy allowed every
command to complete.

The fixed 48-byte little-endian header is:

```text
offset  type    field
0       [u8;4]  magic "PUI1"
4       u16     version = 1
6       u16     header bytes = 48
8       u32     total stream bytes
12      u16     command count
14      u8      cursor icon
15      u8      flags
16      f32     text editor x0
20      f32     text editor y0
24      f32     text editor x1
28      f32     text editor y1
32      f32     primary cursor x0
36      f32     primary cursor y0
40      f32     primary cursor x1
44      f32     primary cursor y1
```

Header flag bit 0 means the pointer is over mutable text. Bit 1 means the two
rectangles contain active IME geometry. Coordinates are surface-relative
logical UI points and both rectangles MUST be finite and ordered. When bit 1 is
clear, bytes 16–47 MUST be zero. Unknown flag bits are invalid.

Cursor values are:

```text
0 default        1 none             2 context-menu     3 help
4 pointing-hand  5 progress         6 wait             7 cell
8 crosshair      9 text            10 vertical-text   11 alias
12 copy         13 move            14 no-drop         15 not-allowed
16 grab         17 grabbing        18 all-scroll      19 resize-horizontal
20 resize-ne-sw 21 resize-nw-se    22 resize-vertical 23 resize-east
24 resize-se    25 resize-south    26 resize-sw       27 resize-west
28 resize-nw    29 resize-north    30 resize-ne       31 resize-column
32 resize-row   33 zoom-in         34 zoom-out
```

Each command immediately follows the previous payload:

```text
offset  type  field
0       u8    opcode
1       u8    flags
2       u16   zero
4       u32   payload bytes
8       [...] payload
```

Version 1 commands are:

```text
opcode  flags  payload
1       0      clipboard text as UTF-8
2       bit 0  non-empty URL as UTF-8; bit 0 requests a new surface
```

Commands are processed in stream order. URL bytes are untrusted input: the Host
MUST apply its navigation scheme, origin, permission, and user-gesture policy,
and a new surface MUST NOT retain a privileged opener. Clipboard access remains
subject to platform policy. Version 1 deliberately has no image-clipboard
command; unknown opcodes are rejected rather than ignored.

A stream is at most 256 KiB and contains at most 64 commands. Clipboard text is
at most 64 KiB and a URL is at most 8 KiB. All reserved bytes and unsupported
flags MUST be zero. The encoded total must end exactly after the declared
command sequence.

Return values:

```text
0  accepted
1  malformed, out-of-bounds, or over-limit stream
2  a stream was already submitted during this call
```

### Motion

```text
host_motion_read(pointer: u32, capacity: u32) -> i32
```

The hostcall is part of the base ABI and MUST always resolve. A Host without a
motion source returns an explicit status instead of leaving the import
unresolved. One successful read consumes the latest sample; later reads return
zero until a newer sample arrives.

```text
 48  one complete MotionSample v1 record written
  0  no newer sample
 -1  motion unavailable
 -2  motion permission denied
 -3  invalid guest output range
 -4  output capacity is smaller than 48 bytes
```

MotionSample v1 is a fixed 48-byte little-endian record:

```text
offset  type    field
0       [u8;4] magic "PMO1"
4       u16    version = 1
6       u16    flags
8       u32    byte length = 48
12      u32    nonzero sequence
16      f64    monotonic timestamp, milliseconds
24      f32    acceleration including gravity X, m/s²
28      f32    acceleration including gravity Y, m/s²
32      f32    acceleration including gravity Z, m/s²
36      f32    rotation rate alpha around Z, degrees/second
40      f32    rotation rate beta around X, degrees/second
44      f32    rotation rate gamma around Y, degrees/second
```

Flags are:

```text
bit 0  acceleration fields are valid
bit 1  rotation fields are valid
bit 2  rotation is emulated from pointer movement
```

All numeric fields MUST be finite. Pointer emulation sets alpha and all
acceleration fields to zero, fills beta and gamma, and sets bits 1 and 2.

An application that cannot operate without motion lists `"motion"` in
`deviceInput.requiredFeatures`. An application with pointer or keyboard
fallback does not require motion and MUST handle `-1` and `-2`.

### Time

```text
host_time_ms() -> u64
host_sleep_ms(duration_ms: u32) -> ()
```

`host_time_ms` returns a monotonic millisecond clock scoped to the execution.
It is not wall-clock time.

`host_sleep_ms` yields or advances runtime time by no more than the remaining
sleep allowance for the current call. A Host MAY return earlier than the
requested duration.

### Audio

```text
host_audio_submit(pointer: u32, sample_count: u32) -> u32
```

Samples are interleaved signed 16-bit little-endian PCM, stereo, at 48,000 Hz.
`sample_count` counts individual channel samples and MUST therefore be even.

Return values:

```text
0  accepted
1  invalid sample count or audio queue limit reached
3  audio capability unavailable for this execution
```

### Assets

```text
host_asset_read(
  name_pointer: u32,
  name_length: u32,
  offset: u32,
  destination: u32,
  capacity: u32
) -> u32
```

The asset name is UTF-8 and relative to the verified application archive. The
Host writes at most `capacity` bytes starting at `offset` and returns the
number written.

Zero means the name was invalid, the asset was absent, or the offset was at or
past the end of the asset. Assets are immutable for the lifetime of one
execution.

### Save data

```text
host_save_submit(pointer: u32, length: u32) -> u32
```

The call submits one opaque save-data value for Host persistence. A later
successful submission replaces the pending value.

```text
0  accepted
1  empty or over the size limit
```

Storage lifetime, synchronization, and user controls are Host policy outside
this ABI.

### Logging

```text
host_log(pointer: u32, length: u32) -> ()
```

The Host copies at most the v1 log-byte limit and decodes the bytes as lossy
UTF-8 for diagnostics. Logs are not application storage and MUST NOT affect
application behavior.

## ABI v1 resource bounds

The initial v1 implementation applies the following ceilings:

```text
program bytes                         64 MiB
read-write data                       64 MiB
stack                                 16 MiB
heap                                  128 MiB
asset files                           2,048
one asset                             128 MiB
all assets                            256 MiB
one asset read                        16 MiB
Host-call bytes per init/update       32 MiB
Host calls during init                131,072
Host calls during update              65,536
sleep during init                     100 ms
sleep during update                   50 ms
audio samples per submission          96,000
queued audio                           2 seconds
queued input events                   4,096
save data                             1 MiB
one log                               4 KiB
queued logs                           64
queued GPU batches                    4
queued GPU events                     256
GPU submissions per init/update       8
GPU inline uploads per init/update    16 MiB
TrUAPI frame                          1 MiB
queued TrUAPI frames per direction   32
queued TrUAPI bytes per direction    4 MiB
```

Profile contracts define their additional bounds. Conforming Hosts MUST NOT
accept values above these ceilings. Before this draft becomes stable, the Host
SDK maintainers must decide which values are also minimum capacities that every
conforming Host must provide.

## Failure and shutdown

A successful `init` does not guarantee that later updates will succeed. The
Host stops the execution on an unhandled guest trap, gas exhaustion, invalid
memory access, unrecoverable profile error, or Host transport failure.

The Host may stop an execution when its App surface closes, the Product is
replaced, or platform lifecycle policy requires termination. ABI v1 does not
promise transparent restoration of guest memory or graphics resources after a
stop.

Device loss and recoverable WebGPU errors are delivered according to the
selected WebGPU event contract. They do not permit stale resource handles to
be reused.

## Version compatibility

A Host that does not implement PolkaVM application ABI v1 MUST NOT launch an
App requesting it. A program compiled for ABI v1 imports only the symbols and
uses only the behavior defined by this document and its selected capability
contracts.

Changes to an import signature, lifecycle requirement, record layout, or
observable status meaning require a new ABI version unless explicitly defined
as a backward-compatible extension.

## Conformance

The normative fixture set contains reproducible PolkaVM guests and expected
results covering:

- required exports and initialization;
- repeated updates;
- guest traps and gas exhaustion;
- invalid guest-memory ranges;
- input record delivery;
- monotonic time;
- asset reads;
- audio submission and gating;
- save submission;
- bounded logging;
- TrUAPI request/response round trips and queue bounds;
- graphics-profile enforcement.

Native and browser implementations MUST run the same fixture inputs. Full
sample applications are integration evidence rather than normative fixtures.

## Open questions before stabilization

- Which resource values are required minimum capacities across all Hosts?
- What stable registry defines key and pointer-button codes?
- Is `host_sleep_ms` necessary in the stable cooperative ABI, or should the
  Host own all scheduling without a guest sleep operation?
- Should save persistence be a capability declared separately from the base
  ABI?
- How is the CoreVM compatibility path named and versioned independently from
  this cooperative ABI?
- Does guest-armed pointer capture belong in the base ABI as `host_pointer_capture`,
  or in a versioned input extension negotiated separately? The call ships
  provisionally and needs the standardisation RFC before Hosts advertise it.
