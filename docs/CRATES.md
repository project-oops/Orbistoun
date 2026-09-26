# Crates

A Cargo workspace. `Cargo.toml` is the authoritative member list; this page says what each crate
is for.

## The dependency spine

Each crate here is required by everything after it, which is also the order they build in.

| Crate | Role |
|-------|------|
| [orbistoun-core](../crates/orbistoun-core/) | Domain types - guest error codes, handles, ABI primitives. IO-free. |
| [orbistoun-env](../crates/orbistoun-env/) | Every environment variable Orbistoun reads, declared once. |
| [orbistoun-elf](../crates/orbistoun-elf/) | Vendor ELF/PRX container parsing. No `unsafe` - `zerocopy` throughout. |
| [orbistoun-nid](../crates/orbistoun-nid/) | NID hashing and symbol-name resolution. |
| [orbistoun-mem](../crates/orbistoun-mem/) | Guest address space - fixed-address reservation, direct and flexible memory. |
| [orbistoun-hle](../crates/orbistoun-hle/) | Module registry, the `guest_module!` macro, stub policy. |
| [orbistoun-loader](../crates/orbistoun-loader/) | Parse, reserve, resolve, relocate, TLS, entry. |

## Execution and the guest OS

| Crate | Role |
|-------|------|
| [orbistoun-abi](../crates/orbistoun-abi/) | The guest-to-host call boundary. |
| [orbistoun-thunk](../crates/orbistoun-thunk/) | Per-import thunks: the machine code a guest lands on, and the dispatch behind it. |
| [orbistoun-kernel](../crates/orbistoun-kernel/) | Guest kernel - memory syscalls, threads, synchronisation. |
| [orbistoun-firmware](../crates/orbistoun-firmware/) | A skeleton of the hardware's firmware address space, for guests that read raw memory past the named interface. |
| [orbistoun-libc](../crates/orbistoun-libc/) | The C library as the guest calls it. |
| [orbistoun-posix](../crates/orbistoun-posix/) | The POSIX-named half of the platform, delegated to what already implements it. |
| [orbistoun-fs](../crates/orbistoun-fs/) | Guest filesystem - file IO and async streaming. |
| [orbistoun-shell](../crates/orbistoun-shell/) | The system software: session lifecycle, guest-visible events, system settings. |
| [orbistoun-systemservice](../crates/orbistoun-systemservice/) | The settings and status a title asks the system about. |
| [orbistoun-video](../crates/orbistoun-video/) | Video output - swapchain and flips. |
| [orbistoun-audio](../crates/orbistoun-audio/) | Audio output. |
| [orbistoun-net](../crates/orbistoun-net/) | Networking HLE: declarations for the libraries a title imports. |
| [orbistoun-input](../crates/orbistoun-input/) | Controller input. |

## Graphics

| Crate | Role |
|-------|------|
| [orbistoun-gpu](../crates/orbistoun-gpu/) | Command-stream translation. No dependency on any host graphics API. |
| [orbistoun-gpu-vulkan](../crates/orbistoun-gpu-vulkan/) | The only crate that knows Vulkan exists. |
| [orbistoun-shader](../crates/orbistoun-shader/) | Guest shader bytecode: decoding it, and measuring how much is understood. |
| [orbistoun-translate](../crates/orbistoun-translate/) | Decoded guest shaders to SPIR-V. |
| [orbistoun-spirv](../crates/orbistoun-spirv/) | SPIR-V module construction. Knows nothing about the guest. |

## Knowledge, tooling and shells

| Crate | Role |
|-------|------|
| [orbistoun-llm](../crates/orbistoun-llm/) | Local-first language-model access: a generic question in, an answer out. Depends on nothing else in the workspace. |
| [orbistoun-names](../crates/orbistoun-names/) | Generating and confirming candidate symbol names. |
| [orbistoun-propose](../crates/orbistoun-propose/) | Proposals paired with the oracle that checks them - where a model meets the hash. |
| [orbistoun-turn](../crates/orbistoun-turn/) | The steps of a loop turn that need no person: what a finding calls for, the sweeps that answer it, and what a turn earned. |
| [orbistoun-report](../crates/orbistoun-report/) | Run reports, traces, the progress verdict and the ranked findings. |
| [orbistoun-probe](../crates/orbistoun-probe/) | Reading the records a hardware conformance probe produces. |
| [orbistoun-overrides](../crates/orbistoun-overrides/) | Per-title settings and compatibility entries, layered and merged. |
| [orbistoun-submit](../crates/orbistoun-submit/) | What one machine contributes - gathered, and checked by re-derivation rather than trusted. |
| [orbistoun-corpus](../crates/orbistoun-corpus/) | The test corpus: a manifest of sources, pinned release assets fetched into the title library. |
| [orbistoun-paths](../crates/orbistoun-paths/) | Portable-first path resolution. Never writes outside its own root. |
| [orbistoun-proto](../crates/orbistoun-proto/) | The shim-to-worker protocol: messages as data. |
| [orbistoun-service](../crates/orbistoun-service/) | The shared logic layer every shim calls. |
| [orbistoun-gen](../crates/orbistoun-gen/) | Offline generators for the shader data tables. Not part of the emulator. |
| [orbistoun-worker](../crates/orbistoun-worker/) | The isolated process a guest runs in. |
| [orbistoun-cli](../crates/orbistoun-cli/) | The `orbistoun-cli` binary. |
| [orbistoun-gui](../crates/orbistoun-gui/) | The desktop shell. |

## Shims

`orbistoun-cli`, `orbistoun-gui` and worker mode are **shims**: none is privileged, and none holds
behaviour the others lack. Adding a subsystem is one `guest_module!` declaration and one line in
`modules()` in [orbistoun-service/src/symbols.rs](../crates/orbistoun-service/src/symbols.rs).
