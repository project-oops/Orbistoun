# D789 - The global data share is one zeroed buffer a process keeps

**Status:** decided
**Date:** 2026-10-11

A compute program's `ds_write_b32` and `ds_read_b32` with the GDS bit (bit 17 of the first word on
GFX10, `aco_assembler.cpp:513`) reach the global data share: one buffer of
[`GDS_WORDS`](../../crates/orbistoun-spirv/src/lib.rs) words, zeroed when the device session opens,
bound at `GDS_BINDING` of set zero to every compute module that declares it, and kept until the
process ends. A word is addressed by the lane's byte address plus the instruction's offset. It is
stored or loaded only when it lies wholly within `m0`, which for a GDS access is the share's size
in bytes; past it a store is dropped and a load reads zero.

`m0`'s meaning is LLVM's, used as an oracle (`known_by = published`): on `gfx1013` it sets `m0`
from the function's `amdgpu-gds-size` before a GDS access - `s_movk_i32 m0, 0x100` for 256 bytes,
`s_mov_b32 m0, 0xc000` for 49152. PPSA25872's first dispatch sets `0xc000` with
`s_bfm_b32 m0, 2, 14`.

Assumed (`known_by = assumed`): that the share starts zeroed, that it outlives a dispatch and a
submission within a process, that 64 KiB is enough (the program here declares 48), and that an
access past `m0` is dropped rather than wrapped. Other GDS instructions - the pair, byte and
atomic forms, and `ds_ordered_count` - and a GDS access outside a compute program stay refused by
name.

**Why:** the user chose a process-wide buffer on 2026-10-11 over a share per submission and over
leaving GDS refused. A per-submission share would lose what a title sets up once and reads later.

**Rejected:**
- Carrying the access out on the workgroup's local data share, as the translation did before
  2026-10-11: the write vanished with the workgroup, and nothing said so.
- A share per submission: simpler, but it forgets state a title carries between submissions.
