# Crates

A Cargo workspace. `Cargo.toml` is the authoritative list; this says what each crate is *for*,
which is the part a reader needs and the part that does not go stale.

This was the README's "Workspace layout" section. It is reference for somebody already working
in the tree rather than something a person arriving needs before anything else.

A Cargo workspace. `Cargo.toml` is the authoritative list; the table below says what each
crate is *for*, which is the part a reader needs and the part that does not go stale.

The crates at the head of it are a **dependency spine** - each is required by everything
after it, which is also the order they get built in.

| Crate | Role |
|-------|------|
| [orbistoun-core](../crates/orbistoun-core/) | Domain types - guest error codes, handles, ABI primitives. IO-free. |
| [orbistoun-env](../crates/orbistoun-env/) | Every environment variable orbistoun reads, declared once. |
| [orbistoun-elf](../crates/orbistoun-elf/) | Vendor ELF/PRX container parsing. No `unsafe` - `zerocopy` throughout. |
| [orbistoun-nid](../crates/orbistoun-nid/) | NID hashing and symbol-name resolution. |
| [orbistoun-mem](../crates/orbistoun-mem/) | Guest address space - fixed-address reservation, direct/flexible memory. |
| [orbistoun-hle](../crates/orbistoun-hle/) | Module registry, the `guest_module!` macro, stub policy. |
| [orbistoun-loader](../crates/orbistoun-loader/) | Parse → reserve → resolve → relocate → TLS → entry. |

**Execution and the guest OS**

| Crate | Role |
|-------|------|
| [orbistoun-abi](../crates/orbistoun-abi/) | The guest-to-host call boundary, proved end to end. |
| [orbistoun-thunk](../crates/orbistoun-thunk/) | Per-import thunks: the machine code a guest lands on, and the dispatch behind it. |
| [orbistoun-kernel](../crates/orbistoun-kernel/) | Guest kernel - memory syscalls, threads, synchronisation. |
| [orbistoun-libc](../crates/orbistoun-libc/) | The C library as the guest calls it. The largest implemented surface. |
| [orbistoun-fs](../crates/orbistoun-fs/) | Guest filesystem - file IO and async streaming. |
| [orbistoun-systemservice](../crates/orbistoun-systemservice/) | The settings and status a title asks the system about. |
| [orbistoun-video](../crates/orbistoun-video/) | Video output - swapchain and flips. |
| [orbistoun-audio](../crates/orbistoun-audio/) | Audio output. |
| [orbistoun-input](../crates/orbistoun-input/) | Controller input. |

**Graphics**

| Crate | Role |
|-------|------|
| [orbistoun-gpu](../crates/orbistoun-gpu/) | Command-stream translation. No dependency on any host graphics API. |
| [orbistoun-gpu-vulkan](../crates/orbistoun-gpu-vulkan/) | The only crate that knows Vulkan exists. |
| [orbistoun-shader](../crates/orbistoun-shader/) | Guest shader bytecode: decode it, and measure how much is understood. |
| [orbistoun-translate](../crates/orbistoun-translate/) | Decoded guest shaders → SPIR-V. |
| [orbistoun-spirv](../crates/orbistoun-spirv/) | SPIR-V module construction. Knows nothing about the guest. |

**Knowledge, tooling, and shells**

| Crate | Role |
|-------|------|
| [orbistoun-llm](../crates/orbistoun-llm/) | Local-first language-model access, as a generic question in and an answer out. Depends on nothing else here, on purpose. |
| [orbistoun-names](../crates/orbistoun-names/) | Generating and confirming candidate symbol names. |
| [orbistoun-propose](../crates/orbistoun-propose/) | Proposals paired with the oracle that checks them - where a model meets the hash. |
| [orbistoun-report](../crates/orbistoun-report/) | Run reports, traces, the progress verdict, and the ranked findings. |
| [orbistoun-probe](../crates/orbistoun-probe/) | Reading the records a hardware conformance probe produces. |
| [orbistoun-overrides](../crates/orbistoun-overrides/) | Per-title settings and compatibility entries, layered and merged. |
| [orbistoun-paths](../crates/orbistoun-paths/) | Portable-first path resolution. Never writes outside its own root. |
| [orbistoun-proto](../crates/orbistoun-proto/) | The shim-to-worker protocol: messages as data. |
| [orbistoun-service](../crates/orbistoun-service/) | The shared logic layer every shim calls. |
| [orbistoun-worker](../crates/orbistoun-worker/) | The isolated process a guest actually runs in. |
| [orbistoun-cli](../crates/orbistoun-cli/) | The `orbistoun` binary. |
| [orbistoun-gui](../crates/orbistoun-gui/) | The desktop shell. |

`cli`, `gui`, and worker mode are **shims** - none is privileged and none holds
behaviour the others lack. Adding a subsystem means one `guest_module!` declaration and
one line in `modules()` in
[orbistoun-service/src/symbols.rs](../crates/orbistoun-service/src/symbols.rs). Nothing
else changes.
