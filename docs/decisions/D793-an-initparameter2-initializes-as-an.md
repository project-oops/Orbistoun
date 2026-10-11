# D793 - An InitParameter2 initializes as an InitParameter does

**Status:** decided
**Date:** 2026-10-11

`Initializer::initialize(const InitParameter2*)` is carried out exactly as the measured
`Initializer::initialize(const InitParameter*)`: the allocator from the block's first word, every
allocation passed the word at `+8`, the same twelve allocations, the same answers.

The block itself is measured (obSCEne `130-layout/json2-init`, `arm2-init`, `param-after`): the
`InitParameter2` constructor and `setAllocator` leave `0x28` bytes - the allocator at `+0`, the
`setAllocator` word at `+8`, zero but for `2` at `+0x18` - and the `InitParameter` form of
`initialize`, handed that block, made its twelve allocations through it and answered 0.

Assumed (`known_by = assumed`): that the `InitParameter2` form reads the block the same way. The
probe called only the other form. `setFileBufferSize`, whose field is unmeasured, stays unserved.

**Why:** PPSA02664 and PPSA03416 both build an `InitParameter2` and initialize through it; each
stops at the constructor's placeholder.

**Rejected:**
- Waiting for the second form to be measured: the block is measured, and the guess is only
  which overload reads it.
