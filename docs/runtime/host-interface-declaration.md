---
title: "Declaring required Polkadot Host interfaces"
type: rfc-draft
status: draft — input for the TruAPI RFC process
---

# Declaring required Polkadot Host interfaces

Draft for a TruAPI RFC. Nothing here is stable until the RFC lands; the
experimental hosts implement it as a prototype.

## Motivation

A PolkaVM program blob does not self-describe which host contract it
expects: cartridge guests (application ABI v2) and computer guests
(`polkadot-host` interfaces) are both PolkaVM bytecode exporting
`_pvm_start`. Hosts must decide the execution model and the authority
grant *before* running anything, and users must be able to see what an
app requests.

Two rejected alternatives frame the design:

- **A new manifest/runtime kind** (e.g. `polkavm-computer`) duplicates
  what the capability request already says, and forces consistency
  validation between the kind and the capability list.
- **A new package container** for program-plus-metadata is unnecessary:
  the `.polkavm` container already supports optional custom sections
  that all existing parsers skip (`polkavm-common` `program.rs`: any
  section id with the high bit set is length-prefixed and skippable;
  ids 128–131 are reserved for debug data and the metadata hash).

Therefore: one small declaration record, carried by the executable manifest (and optionally mirrored in the blob for tooling).

## The declaration record

A canonical UTF-8 JSON object:

```json
{
  "requires": [
    "polkadot-host/0.1/core",
    "polkadot-host/0.1/fs",
    "polkadot-host/0.1/tty",
    "polkadot-host/0.1/process"
  ]
}
```

- `requires` is a non-empty list of unique interface ids.
- Interface ids follow the ADR namespace `polkadot-host/<version>/<name>`.
  The contract version rides on the id; there is no separate ABI version
  field anywhere in the declaration.
- `polkadot-host/0.1/core` is mandatory: every conforming guest needs
  arguments, environment, and exit.
- Future extensions attach per-interface parameters as sibling keys
  (e.g. filesystem path grants); the RFC must reserve that shape.

## The executable manifest

App Manifest v2 apps declare the record under `capabilities.host`:

```json
{
  "$v": 2,
  "kind": "app",
  "runtime": { "kind": "polkavm", "entrypoint": "shell.polkavm" },
  "capabilities": {
    "host": { "requires": ["polkadot-host/0.1/core", "…"] }
  }
}
```

- `runtime.kind` stays `polkavm`; the presence of `capabilities.host`
  selects the host-interface execution model.
- `runtime.abiVersion` MUST be absent (versions live in interface ids).
- Graphics/device-input/audio capability blocks MUST be absent; those
  belong to the cooperative application ABI. When a graphics interface
  exists (`polkadot-host/<v>/display`), it will be an interface id.
- This carrier is the pre-fetch trust anchor: it is published as the
  DotNS executable record, byte-verified against the packaged
  `manifest.json`, and evaluable before any content is fetched.

## Spawning published apps (open spawn)

`process_spawn(name)` is not gated on any manifest enumeration. The name
resolves like any app launch: the host looks up the DotNS label, the
fetched archive is verified against the child's OWN signed executable
record, the child must declare a host contract the host supports, and
its grant clamps to the parent's. The child is a first-class published
app — the same artifact serves `vim.example` standalone and every
computer that spawns it, deduplicated by CID.

Two qualifications:

- **Registry reach is host-mediated.** Spawning by name lets a guest
  trigger resolutions and content fetches, which is authority (and a
  covert channel: data can be encoded in which names are looked up).
  The host owns the policy: prompt, install-on-first-use, cache-only,
  or unrestricted on development hosts. An unresolvable or refused name
  fails the spawn with NOT_FOUND; the guest observes nothing else.
- **Pins are an optional lockfile, not authorization.** A manifest MAY
  carry `packages` entries pinning a spawn name to a CID (and MAY bundle
  program files archive-locally for fixtures and private helpers). Pins
  buy determinism, offline closure, and supply-chain stability — like a
  `Cargo.lock`, they never grant or deny anything.

## A `.polkavm` custom section (optional tooling)

The same declaration record MAY be embedded in a program blob as an
optional custom section (id from the skippable `0x80..0xFF` space,
payload = the canonical JSON bytes). Existing loaders, the wasm
translator, and gas metering ignore unknown optional sections, so
stamped blobs run unchanged everywhere. This is tooling and defense in
depth, not a required carrier: `polkatool`-style tools can print what a
blob requires, archive-local programs that never pass through DotNS can
self-describe, and hosts MAY refuse on a manifest/blob mismatch.

## Host rules

1. **Fail closed.** A host MUST refuse to launch a program requiring an
   interface it cannot or will not provide, before executing anything.
2. **Grants clamp, declarations do not grant.** The record is a request;
   the host decides the actual grant and MAY grant less (e.g. deny
   network) where the interface semantics allow partial refusal.
3. **Unknown ids are errors, not warnings.** Forward compatibility comes
   from versioned ids, not from ignoring requests.
4. **Children clamp to parents.** A spawned child's effective grant is
   at most its own declaration intersected with its parent's grant.
5. **Every resource is host-provided and virtual.** A declaration never
   names host-side resources. "Filesystem" means the virtual namespace
   the host mounts into the process (`/home` in 0.1) — a host-owned map
   behind opaque handles, never the host's real filesystem; the backing
   (IndexedDB record, host-chosen directory, memory) is host policy the
   guest cannot observe, choose, or escape. The same holds for every
   interface: sockets, terminals, and children are host-mediated objects,
   not operating-system resources.

## Open questions for the RFC

- Section id assignment and a registry for interface namespaces.
- Canonical JSON rules (key order, whitespace) so byte-compares work.
- Per-interface parameter shapes: which additional *host-provided virtual
  mounts or targets* a host may offer a process (never guest-named host
  paths), and how apps request persistence.
- Whether the metadata-hash section (131) should commit to the
  declaration bytes.
