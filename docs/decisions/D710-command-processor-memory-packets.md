# D710 - command-processor memory packets execute at submit, stopping at the first packet that needs the GPU

**Status:** assumed
**Date:** 2026-09-24

## The question

D705 decided that a submission's completion is not posted until execution lands: a fence written for
work that never ran claims a result the work did not produce. Both fully-owned baselines (the GL cube,
Neverball) then stalled on their first submit, the GL context's hardware clear self-test, polling a
fence that nothing writes (worklog 813). What does "execution lands" mean for that stream - and does
any of it land before shader execution does?

## The decision

**The command processor's own memory work executes at submit, on the CPU, in stream order, and stops
at the first packet that needs the GPU.** `orbistoun_gpu::cp::execute` carries out:

- `DMA_DATA` fills (data source) and copies (address source), into and out of guest memory;
- `RELEASE_MEM`'s end-of-pipe write - a 32- or 64-bit value, or the clock counter - or its discard;
- `WAIT_REG_MEM` on a memory word (equal, not-equal, greater-or-equal), which either holds or stops
  execution, since nothing runs concurrently that could ever make it hold;

and passes over the packets with no effect on memory: register writes (state for engines that are not
running), no-ops, and cache control (`ACQUIRE_MEM`, a one-dword `EVENT_WRITE`) - there is no cache
between this command processor and guest memory. **Anything else stops it**: a draw, a dispatch, a
register wait, a GDS selector, an address-hold DMA, an opcode not listed. Nothing after the stop
retires, so a `RELEASE_MEM` is reached - its fence written - only when every packet before it has been
carried out or has no effect on memory.

Writes go only where the kernel's live tables say the whole range is writable guest memory
(`orbistoun_kernel::is_guest_writable`, installed by the worker), with touching mappings joined into
one range and any gap or read-only piece refusing it.

## Why this is D705's own condition, not an exception to it

D705 refuses a completion for work that did not run. This runs the work. A fill is a fill whether a
GPU's DMA engine or this loop performs it; the pixels are in guest memory afterwards either way, and a
guest that checks them - the GL context's self-test compares every pixel of its readback against the
clear colour - finds them there because they were written, not because a flag said so. The stop rule
is what keeps D705 whole: a stream with a draw before its fence gets no fence, and the guest's wait on
it stays the honest wall D705 describes, now narrowed to exactly the work that needs shaders.

The timing is synchronous, which D705 already names as the only timing an emulator with no
asynchronous GPU has: the work is done by the time the submit returns.

## What is assumed

- **The clock counter's rate.** `DATA_SEL` 3 writes host nanoseconds since the first stamp, plus one.
  The console's GPU counter rate is unmeasured; the guests here only check that a stamp came back.
- **That cache control has no observable effect here.** True of this memory model by construction; a
  guest that relies on a *stale* cache - reading memory before a flush and expecting old bytes - would
  see new bytes. None is known.
- **The packet layouts** are the public PM4 ones, cited in `cp.rs` from the collection's Mesa tree
  (`sid.h`, `ac_cmdbuf_cp.c`, `gfx103.json`), and agree with what the open-toolchain SDK emits and the
  console executes (the self-test passes on hardware).

## Consequence

The GL cube's clear self-test passes - all 2,073,600 pixels matched, fence `0xbeefcafe`, a timestamp -
and it confirms a frame; the first frame's own submission then stops at `DRAW_INDEX_AUTO` with its fence
unwritten (worklog 816). This retires when shader execution lands for draws (`36c0`): the stop rule
then moves past the draw, and nothing about the memory packets changes.
