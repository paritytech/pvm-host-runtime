---
title: "PolkaVM Tri2D profile wire format v1"
type: runtime-contract
status: draft
---

# PolkaVM Tri2D profile wire format v1

## Scope

This document defines the command stream accepted by `host_tri2d_submit` when an App manifest selects graphics profile `tri2d` with `abiVersion: 1`. All integers are little-endian. Coordinates use a top-left origin with positive X to the right and positive Y down.

A stream is one atomic frame. The Host validates the complete stream and its retained-resource transition before changing texture state or presenting. A rejected stream changes no retained state.

## Frame header

Every stream begins with a 24-byte header:

| Offset | Type | Field |
|---:|---|---|
| 0 | `[u8; 4]` | magic `ETD1` |
| 4 | `u16` | version, `1` |
| 6 | `u16` | header bytes, `24` |
| 8 | `u32` | physical surface width |
| 12 | `u32` | physical surface height |
| 16 | `u32` | command count |
| 20 | `u32` | clear color, bytes packed R, G, B, A from least to most significant |

Width and height are nonzero and at most 4,096. A frame contains 1 through 8,192 commands and is at most 8 MiB.

## Command header

Each command begins with:

| Offset | Type | Field |
|---:|---|---|
| 0 | `u8` | opcode |
| 1 | `u8` | flags, zero in v1 |
| 2 | `u16` | reserved, zero |
| 4 | `u32` | payload byte length |

The payload immediately follows the header and contains exactly the declared number of bytes. Unknown opcodes, nonzero flags, truncated payloads, and trailing frame bytes reject the whole stream.

## Retained textures

Texture handles are nonzero `u32` values selected by the guest. Texture state persists across accepted frames until an explicit destroy. A frame may draw with a texture created in an earlier frame.

A Host retains at most 256 textures and 64 MiB of texture pixels. Each texture dimension is at most 4,096. Texture pixels are tightly packed, row-major, non-sRGB RGBA8 bytes. Vertex colors and texture pixels use premultiplied alpha; Hosts blend with source factor one and destination factor one-minus-source-alpha.

### Opcode 1: texture create

Payload:

| Offset | Type | Field |
|---:|---|---|
| 0 | `u32` | new handle |
| 4 | `u32` | width |
| 8 | `u32` | height |
| 12 | `u32` | filter: `0` nearest, `1` linear |
| 16 | `u32` | pixel byte length |
| 20 | `[u8]` | `width × height × 4` RGBA8 bytes |

The handle must not already exist.

### Opcode 2: texture update

Payload:

| Offset | Type | Field |
|---:|---|---|
| 0 | `u32` | existing handle |
| 4 | `u32` | destination X |
| 8 | `u32` | destination Y |
| 12 | `u32` | update width |
| 16 | `u32` | update height |
| 20 | `u32` | pixel byte length |
| 24 | `[u8]` | `width × height × 4` RGBA8 bytes |

The update rectangle must be nonempty and contained by the existing texture.

### Opcode 3: texture destroy

Payload: one existing nonzero `u32` handle. The handle becomes unavailable to later commands in the same frame and to later frames.

## Draws

### Opcode 4: indexed triangle draw

Payload header:

| Offset | Type | Field |
|---:|---|---|
| 0 | `u32` | existing texture handle |
| 4 | `u32` | physical clip X |
| 8 | `u32` | physical clip Y |
| 12 | `u32` | physical clip width |
| 16 | `u32` | physical clip height |
| 20 | `u32` | vertex count |
| 24 | `u32` | index count |

The nonempty clip rectangle must fit within the frame surface. Vertex and index arrays follow the 28-byte payload header.

Each vertex is 20 bytes:

| Offset | Type | Field |
|---:|---|---|
| 0 | `i32` | physical X in signed 16.16 fixed point |
| 4 | `i32` | physical Y in signed 16.16 fixed point |
| 8 | `i32` | texture U in signed 16.16 fixed point |
| 12 | `i32` | texture V in signed 16.16 fixed point |
| 16 | `[u8; 4]` | normalized premultiplied RGBA vertex color |

The vertex array is followed by `index_count` little-endian `u32` indices. Every index is less than `vertex_count`, and the index count is a nonzero multiple of three.

One frame contains at most 4,096 draw commands, 262,144 vertices, and 786,432 indices in total. Draw order is command order.

## Presentation

### Opcode 5: present

Present has an empty payload and must be the final command. Exactly one present is required. Commands after present reject the stream.

The Host clears the surface using the header color, executes draws in order with clip rectangles applied, and makes the completed frame visible. An accepted frame commits its texture creates, updates, and destroys together.

## Application ABI status

`host_tri2d_submit` accepts at most one stream during one `init` or `update` call and returns:

| Value | Meaning |
|---:|---|
| 0 | accepted |
| 1 | malformed or out of bounds |
| 2 | a stream was already submitted during this call |
| 3 | Tri2D is unavailable for this execution |

The constants and stateful validator for this contract are exported by `polkavm-gpu-wire::tri2d`. Host implementations and guest encoders must consume that shared definition rather than maintain private opcode or limit tables.
