# Worklog

One entry per milestone. Commit messages hold the rest.

## 2026-08-19 - Workspace under the full gate

- The workspace builds under one gate: fmt, clippy at `-D warnings`, tests, doctests, rustdoc
  with broken-link denial, machete, audit and cargo-deny.
- ELF64 header and program-table parsing, NID hashing and reverse lookup, the module registry
  with `guest_module!`, the stub policy and the trace event model exist.
- `orbistoun-core` has no dependencies.
- `cargo-machete` does not scan target-specific dependencies.
- A `[workspace.dependencies.<name>]` sub-table header captures every key declared after it;
  that block uses inline tables.

## 2026-08-19 - Containers read, images placed

- `inspect` reports the structure of real containers; the wrapper's descriptor table, not the
  inner program headers, maps file bytes.
- Imports are standard ELF dynamic machinery with vendor-encoded names.
- `verify` measures a symbol database against a module's imports; `report` persists a run and
  diffs it against the previous run of the same content hash.
- `load` reserves a module as one contiguous span; `run` drives a worker child process that
  places every loadable segment and zeroes `.bss`.
- A 96 MB title executable places at `0x400000000000` inside a worker.
- Windows reserves at 64 KiB granularity and silently rounds an unaligned reservation base down.
- A plain reservation at an explicit base refuses rather than overwrites; Linux needs
  `MAP_PRIVATE` for the same call.
- Title executables link at virtual address zero and need a placement base like any module.

## 2026-08-19 - Guest code executes

- Title executables place, relocate, protect, receive per-import stubs and enter at their entry
  points; the largest applies 174,172 relocations.
- A fault is reported with its operation and address; a time limit turns a spin into a report
  ranked by library and hash.
- A declared function reaches the guest through the registry; the direct-memory query is the
  first implementation.
- Text segments are execute-only (`p_flags == 0x1`), so protection mapping never requires read.
- No executable examined has a writable and executable segment.
- Guests ignore return codes and read their out-parameters.

## 2026-08-19 - A translated instruction executes on a GPU

- `orbistoun-spirv` emits storage-buffer modules; `orbistoun-gpu-vulkan` probes a device,
  dispatches and reads back.
- The gate states whether device tests ran or were skipped.
- A register move translates and leaves its value in the register file on a real device.
- A malformed module can crash the driver instead of failing validation; `spirv-val` runs first.
- A private register file is undefined at entry and needs an explicit null initialiser.

## 2026-08-20 - A submitted command stream runs a shader

- Packets become register writes, a register write names a shader address, and the shader is
  fetched from guest memory, decoded, translated and executed.
- A shader in guest memory has no length; decoding stops at the program-ending instruction.
- The translation cache is keyed on shader bytes, not addresses.
- A compiler-produced shader with scalar compares and branches translates.
- The decoder targets the hardware's GPU instruction set with 64-lane waves, from the published
  reference.
- That reference's field table puts the LDS opcode at `[24:17]`; its own opcode table and
  assembled bytes agree on `[25:18]`.

## 2026-08-20 - A window, and a guest filesystem

- `orbistoun-gui` lists the title library, summarises a title and launches it; the run view shows
  the same verdict, call tail and ranked imports as the CLI.
- Worker mode is checked before any window is created.
- An immediate-mode window repaints on request while a run is in flight.
- `orbistoun-fs` has a mount table and an open-file table; `/app0` maps the title directory
  read-only.
- Path escapes are refused by walking components, never by resolving first.
- C++ allocation and `malloc` share one heap.
- A guest carries an error code onward as a handle; handles are backed by real memory.

## 2026-08-21 - Compatibility records

- `compat/` holds one TOML per title: the settings orbistoun applies and the status it reached.
- The status is derived from a trace, never typed; `compat record` transcribes and `compat list`
  ranks.
- A run that answered blindly is refused as a record.
- Surviving the time limit is an outcome, not a distance.

## 2026-08-22 - The probe bridge calls and reads

- A client sends `call(address, args)` and `read(address, length)` to a serving obSCEne build.
- A well-formed fatal address is executed, not refused; its death arrives as an acknowledgement
  with no result.
- A bad read address answers either `unmapped` or a death, depending on the serving build.
- A byte record with an odd number of hexadecimal digits is refused and counted.

## 2026-08-25 - The loop turns itself

- `orbistoun-cli env` lists every diagnostic environment variable from one declaring crate.
- `turn` takes a plan's mechanical steps unattended: argument sweeps, watchpoints, fills and
  region grants.
- `orbistoun-turn` holds the dispatcher with no dependency path to a model runtime.
- Tier one writes `learned.toml`, absorbed into the stub policy without overriding any hand-written
  entry and never supplying a default return; deleting the file undoes it.
- One unattended turn moves PPSA02664 past a wall that had held through twenty-three eliminations.
- Marked values, bytes that name the field they sit in, locate what a guest reads without a
  watchpoint.

## 2026-08-27 - Open-toolchain payloads run

- Payload ELFs carry `PT_DYNAMIC`, `DT_NEEDED` and a real `.dynsym`; the loader reads
  `DT_GNU_HASH`-only tables.
- `[entry] at` enters a guest at an image-relative address such as `main`, with matching
  `argc`/`argv`.
- The HLE layer owns C library state: `optind`, `errno` and `strerror` buffers.
- A logging-server payload prints its banner and names its next blocker by file and line.
- A library name in a trace means declared, not implemented.

## 2026-08-30 - A payload serves the network and files

- A payload opens a listening socket and an external TCP connection completes against it.
- An FTP payload lists the root's mount points, resolves `realpath` to guest paths and sends a
  file byte for byte.
- Variadic arguments past the registers are read from the stack.
- An import stub is 64 bytes with a sled into the system-call gadget, so a payload that resolves a
  function and adds a small offset lands in the dispatcher.
- The system-call dispatcher runs on the guest's stack: it records, and the reporting layer prints.
- Paths a guest asked for and did not get are recorded; thread stacks are published to dumps at
  thread creation.
- `PT_DYNAMIC` on a title has virtual address zero and sits at the tail of the vendor segment, so
  it is located by file offset.

## 2026-08-31 - obSCEne runs to completion

- obSCEne runs its whole suite to `OBS|end` under orbistoun, test for test against a hardware
  report.
- Video flips complete on submit through a per-port counter.
- `corpus` runs the test corpus; `compat markdown` generates `COMPATIBILITY.md` from the records.
- The firmware version is a profile setting; each title gets a device sandbox from its overlay.
- obSCEne emits each record to both debug output and fd 1, so a raw stream counts each twice.

## 2026-09-01 - Hardware answers matched through the oracle

- The command-packet writer exists beside the walker; the first two dispatch builders answer as
  hardware does.
- Hardware writes one dword more than the documented direct-dispatch packet.
- Flexible memory is its own budget, matched byte for byte to the hardware's answers.
- The virtual-memory query sees the image and the stack; `sysctlbyname` answers `kern.osrelease`.
- Guest threads have a thread pointer and a TLS block, with a Windows `fs`-base backstop.
- PPSA21564 reaches `main`, prints its banner and the current time, and parses its arguments from
  `/app0/args.txt`.

## 2026-09-02 - The C library ported in bulk

- `worklist --static-gap` lists imported and unimplemented functions before any run.
- Bounded string and memory functions, out-of-line atomics, standard streams, recursive locks, the
  C++ runtime, single-precision maths, the `posix_` family, attribute accessors and timed waits
  are implemented.
- Differential cases compare HLE functions against a reference C library, including calls back
  into guest code.
- Spawned guest threads get their own TLS block; PPSA04263 goes from 10,123 to 332,914 calls.
- Packed typed-buffer loads (UINT, SINT, UNORM, SNORM, FLOAT16) translate and are verified on a
  device.

## 2026-09-03 - A title's own modules load, relocate and start

- `exports` reads what a module provides from its dynamic symbol table.
- A title's shipped modules are found by name, placed, share one stub table with a range per
  module, and relocate with nothing unresolved.
- Starting a module runs its `DT_INIT` and `DT_INIT_ARRAY` in ABI order.
- PPSA02664 executes its own module code and reaches its frame loop: 161 distinct imports and
  20,000,000 calls with no fault.
- A fault at one of orbistoun's own placeholder values is reported as a placeholder, not as an
  emulator bug.

## 2026-09-04 - The translator draws

- A framebuffer oracle clears red and draws blue through hand-assembled shaders, so failure is a
  different colour rather than an absent one.
- A translated colour export draws, compared pixel for pixel with the hand-assembled shader.
- Interpolation instructions translate.
- The first hardware-captured command packets arrive: a header of type, count and opcode with a
  packet of count plus two dwords closes on the measured lengths.
- One builder call can emit several packets.

## 2026-09-11 - A GPU call implemented from measurement

- The shader-creation call writes the measured object model: the object is the guest's header
  region, with three fields filled and every other byte left as the guest wrote it.
- PPSA28061 advances on it, not on a diagnostic.

## 2026-09-14 - Shaders the hardware ran translate and draw

- A pixel shader captured from the hardware translates to a module `spirv-val` accepts, and draws.
- The flat-access no-base scalar field is `0x7d` on this target, confirmed by the reference
  assembler.
- Modules declaring 16-bit types enable the matching device features, and fragment storage writes
  enable `fragmentStoresAndAtomics`; draws are clean under the Vulkan validation layer.
- A primitive shader translates into a mesh module and draws.
- The guest-memory window has a base, and a captured vertex program and pixel shader draw one frame
  together.
- Sampled images, views, samplers and descriptors draw on a device.

## 2026-09-15 - A captured command stream draws a frame

- A captured stream is walked, its shaders found, translated and drawn, with each stage taken from
  the pipeline's attribution.
- Indexed draws are decoded from their own packet body.
- The retail direct-memory pool is one twelve-gibibyte pool, shared by main and plain allocations.
- A user-space `int 0x41` on hardware raises a signal and does not return.

## 2026-09-16 - The executor runs a frame

- `drive` makes each module resident, carries out commands in order and presents, returning
  resident, executed, refused and presented counts.
- A refused command is counted and the frame continues; a residency or device error ends it.
- Compute dispatches write back to guest memory; vertex, mesh and indexed draws execute into the
  guest's target size and viewport.
- A composed submission of target, viewport, shaders and draw runs through one backend.

## 2026-09-17 - Detiling and the submit handover

- The `64KB_R_X` 32-bpp swizzle and its inverse are implemented; other formats are refused.
- Hardware measures texel `(15, 15)` at byte 4348 and `(32, 21)` at byte 2640, bijective over all
  16,384 texels of a 128x128 block.
- Colour target, texture tiling mode, depth, stencil and blend state decode.
- The command-buffer submit reads a 16-byte descriptor `{gpu_addr: u64, size_dwords: u32,
  flags: u8, pad}`, walks the buffer and reports it.

## 2026-09-19 - A hardware-drawn triangle reproduced

- A triangle the hardware drew renders through the backend with every one of its 512 drawn texels
  exact; only the clear colour differs.
- The point twin renders as a point; a point packs its primitive index in the same field a
  triangle uses.
- The `presented` rung is awarded when a flipped buffer reads back written.
- A rendered frame's bytes cross the frame route intact.
- Run records carry the commit they were built from, with `-dirty` for an uncommitted tree.
- An autonomous loop halts on an architectural wall, a spin deadlock or a regression.
- CI is green on Windows, Linux and macOS; macOS tests as `x86_64-apple-darwin`.

## 2026-09-21 - Retail walls move on served data

- PPSA04263 opens `/app0/rpf.cache` from its title root, which clears its `int 0x41` abort.
- A function returning a pointer to orbistoun-owned zeroed memory ships as a `region` knowledge
  entry; PPSA28061 moves past its register-defaults query.
- Retail titles run on hardware, so every wall on one is orbistoun's until measured otherwise.
- A title's files are in the resolved data directory, not in the repository.

## 2026-09-22 - The first fully-owned guest runs

- GLCB00001 links, resolves its imports and enters: a module built the platform's way carries
  only the vendor dynamic tags, with a `DYNAMIC` segment at virtual address zero.
- The system-call gadget is served at `0x8000004ea`, the fixed address a payload uses when it
  cannot resolve one by name.
- Scanout buffers map; the second buffer-registration call reads an array of 32-byte buffer
  structures and has its own handler.
- Queue creation returns a handle, which turns on the guest's hardware path.
- GLCB00001 runs its render loop to `flipped`, 150,996 calls.

## 2026-09-24 - The GL clear self-test passes

- NVRB00001 joins the library with its data tree, staged beside GLCB00001.
- A `.` path component resolves to the directory it sits in.
- The command processor carries out fills, copies, fence writes and memory waits on the CPU at
  submit, and stops at the first packet that needs the GPU.
- The GL clear self-test passes on both baselines, every pixel matched.
- System calls save their registers on the calling thread's own stack.
- Backend refusals are tallied by reason and the run report says why a shader did not translate.

## 2026-09-24 - Fully-owned guests draw

- A live submission's window sits at the 64-bit base its vertex shader forms.
- User data reaches translated shaders through a 32-word push-constant block, sixteen words per
  stage.
- Each draw samples up to two of the guest's textures and applies its blend state.
- Draws run at submit and are tiled back into the guest's target, so fences retire from work that
  ran.
- GLCB00001 draws all twelve faces; NVRB00001 renders its title scene and menu.
- A texture level copy sized beyond its staging buffer loses the device.

## 2026-09-24 - Titles shown and launched from the window

- A flip is detiled into an eight-slot frame ring, throttled to one frame per 50 ms, and shown in
  the window while the run continues.
- The window's runs have no time limit and no call budget.
- SCSH00001 lists the library and launches titles from it: directory reads, a system-wide overlay
  and `/user/app/<id>` mounts by `param.json` id.
- Mount resolution takes the most specific prefix.
- Every oops-apps title, utility, test title and payload is a corpus source.
- An oops-apps package can be a tar archive under a `.zip` name; unpacking tells them apart by
  their bytes.

## 2026-09-24 - Frame rate measured and raised

- A performance overlay shows where a running title's time goes; headless runs print the same
  report.
- One pipeline and one backend last the run; draws on one attachment share one render pass.
- A fragment module simulates one lane, each stage translates at the wave width the stream
  declares, and a known execution mask emits only its lanes.
- Consecutive draws sharing state batch into one mesh dispatch.
- A worker whose parent dies exits.
- NVRB00001 goes from about one frame a second to fifteen.

## 2026-09-25 - Input captured and replayed; NVRB00001 plays

- Pad input is captured and replayed against flip count, from the toolbar only, never
  automatically; `--playback` arms a capture for the first launch.
- A signed-off capture takes NVRB00001 in-game headless at 27 to 28 flips a second.
- A title staged under `data/homebrew/<id>` gets a writable `/app0` by copy-up; storage origin
  decides, never package metadata.
- NVRB00001 plays a level to its end state with no fault.
- A fault report names the faulting page's host state, protection and page-guard history.
- The window runs guests on the host clock; headless runs stay on the logical clock.

## 2026-09-26 - Measured answers move retail titles

- The file-path resolve writes `u32` ids, `u64` sizes and `u32` statuses, answers an unresolved
  path with id `0xffffffff`, and stops at its first miss.
- Only `/app0` paths are valid to that resolve on hardware.
- The shader-creation call relocates every offset self-relative and patches the program address
  into its register descriptor.
- The mapper-parameter call fills its measured 56-byte structure; PPSA28061 goes from 25 to 61
  distinct imports.
- The virtual-memory query writes the whole 72-byte structure; POSIX file calls fail with
  `errno`.
- Watchpoints follow every guest thread, execute breakpoints count their own hits, and a peek can
  start from a fault register.
