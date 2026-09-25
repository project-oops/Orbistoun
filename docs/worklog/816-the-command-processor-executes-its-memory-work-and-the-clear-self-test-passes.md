# 816. The command processor's memory work executes at submit, and the GL clear self-test passes on both baselines — every pixel matched, the fence written by work that ran — verdict FURTHER

**2026-09-24** — worklog 815 showed the cube's clear self-test walking to 192 packets and 0 draws. The
self-test is not a draw at all: `gl_hw_clear` emits `DMA_DATA` fills of the colour, depth and stencil
surfaces, and the trailer closing every stream is a `RELEASE_MEM` fence, a `WAIT_REG_MEM` on it, a
`DMA_DATA` copy of the target into a readback buffer, and a `RELEASE_MEM` of the clock counter. None of
it needs a shader. All of it is exactly reproducible on the CPU.

## What was built

- **`orbistoun_gpu::cp`** - `execute(stream, memory)` walks a stream in order and carries out
  `DMA_DATA` fills and copies, `RELEASE_MEM` value/timestamp writes and memory `WAIT_REG_MEM` checks;
  passes over register state, no-ops and cache control; and **stops at the first packet that needs the
  GPU**, so nothing after it retires and no fence is written for a draw that did not run. Layouts are
  cited from the collection's Mesa tree (`sid.h:90-94, 163-167, 177-184`, `ac_cmdbuf_cp.c:216-229`,
  `gfx103.json:12266` for the 26-bit byte count this generation uses). Three tests: the self-test's
  stream shape runs to its fence and its readback holds the fill's colour; a draw before the fence
  leaves the fence unwritten; an unsatisfiable wait and an unmapped target both stop execution.
- **The submit runs it**: `submit_described` executes each submission's command-processor work after
  walking it, synchronously, and records how many ran to completion; the run report prints it.
- **Writes only where the kernel says**: `orbistoun_kernel::is_guest_writable`, installed by the worker
  beside last tick's readability check. A write is vouched for only by a runtime mapping whose
  protection allows it.
- **Adjacent mappings are one range.** The first run stopped at the very first fill, out of bounds: the
  SDK maps its scanout surface as sixteen 2 MiB pieces end to end, and a fill of the 1080p target crosses
  four of them, while the check wanted one region to cover it. `guest_range_allows` now walks touching
  regions, each of which must grant the access; a gap or a read-only piece refuses the whole range
  (test `adjacent_mappings_are_one_range_and_a_gap_or_a_read_only_piece_is_not`).
- **D710** records the mechanism and why it is D705's own condition met rather than an exception to it.

## What the guests do now

The cube:

```
[OOPS-GL] hw-clear-test-matched: 0x1fa400      <- all 2,073,600 pixels
[OOPS-GL] hw-clear-test-fence: 0xbeefcafe
[OOPS-GL] hw-clear-test-pass: 0x1
[OOPS-GL] frames-confirmed: 0x1
orbistoun: the command processor carried out 1 of 2 submission(s) to completion (35389452 bytes written);
  the last stopped at byte 0x9e8: opcode 0x2d needs the GPU, so nothing after it retired
orbistoun: a submission reached the ... backend: 16 command(s) driven, 0 refused, frame 1920x1080, in 828 ms
verdict  FURTHER  (21 imports, +7)
```

The self-test passes on its own terms - the GL context compares every pixel of its readback against
the clear colour, and they are there because the fill really wrote them and the copy really moved them.
The first frame's submission then stops, honestly, at `DRAW_INDEX_AUTO` (`0x2d`): the draw needs shader
execution, so its fence stays unwritten and the guest waits on it, the wall D705 describes, now exactly
as wide as the work that needs shaders. The render step drives that draw submission to the Vulkan
backend after the run: sixteen commands, none refused, a 1920x1080 frame, in 828 ms.

Neverball:

```
[OOPS-GL] hw-clear-test-pass: 0x1
verdict  FURTHER  (47 imports, +24; 555 calls)
guest fault: write to 0x0 while executing at 0x40000025f06a (image+0x25f06a)
```

Its self-test passes too, and it goes on to reach 24 more imports than last run before a null write at
`image+0x25f06a` on a guest thread - a new wall, orbistoun's by default (D708), and the next unit.

## Where this leaves the rendering cluster

The command processor's side of a submit now executes. What stands between a draw and a retired fence
is shader execution at submit (`36c0`) - the backend already renders the cube's draw submission, after
the run, in under a second. `REQ-...0ab6` (writing a rendered frame back into the guest's colour target)
is still open; D710 settles the part of its question that is a principle - on the target this emulates
the GPU writes guest memory, and orbistoun doing so for work it really performed is faithful.

## Gate state

`crates/orbistoun-gpu/src/cp.rs` (new), `agc_driver.rs` (the write lookup, `GuestCp`, the execution
record, the call in `submit_described`), `lib.rs` (the module); `crates/orbistoun-kernel/src/lib.rs`
(`is_guest_writable`, `guest_range_allows` with adjacency, a test); `crates/orbistoun-worker/src/lib.rs`
(the install), `render.rs` (the execution line); `docs/decisions/D710`. `orbistoun-gpu` 95 and the
kernel tests pass. `./bin/orbistoun check` green, decision and worklog indexes regenerated, identity
scan clean. No commit.
