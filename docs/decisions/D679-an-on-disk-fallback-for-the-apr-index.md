# D679 - an on-disk fallback for the APR index: tried, and not shipped

**Status:** decided
**Date:** 2026-09-12

## The choice

Do **not** ship an on-disk fallback for `sceKernelAprResolveFilepathsToIdsAndFileSizes`. It was built
and run as an experiment (operator-approved) to try to move PPSA25872 (Terminator 2D) and to measure
the unmeasured out-parameter slot layout (D592). It did neither, so it was reverted rather than kept.

## What was tried

`look_up_in_index` (orbistoun-worker) answers `sceKernelApr*` from the title's `ampr_emu.index`.
Terminator ships no such index, so every resolve returned the placeholder. The experiment added a
fallback: for a path not in the index but present on disk (via the mount table), answer the **real**
on-disk size and a synthesized, stable handle-id above any index position - explicitly softening D587's
"report, don't guess outputs", because the read path that would consume the id is unimplemented so the
id stays opaque. Then the six `ORBISTOUN_APR_ANSWER` permutations were swept against Terminator.

## What it showed

- **The fallback works mechanically.** `/app0/Media/RuntimeInitializeOnLoads.json` (which exists on
  disk, 13 bytes) resolved, and `sceKernelAprResolveFilepathsToIdsAndFileSizes` returned `0x0` instead
  of the placeholder.
- **It did not move Terminator, on any of the six permutations.** All still fault at the same `int 0x41`
  (`image+0x17554a3`), 321,96x calls, and the APR-resolve call site (`0x3aa49e`) is **not in the fault
  chain**. So Terminator's `int 0x41` assert is a separate failure, and worklog 517's attribution of it
  to the APR resolve was wrong - the placeholder return was incidental.
- **The slot layout stayed unmeasured.** Terminator gives no oracle (the "Unknown error occurred while
  loading" message D592 relied on for PPSA02664 never appears here, and the assert fires regardless of
  the permutation), so the six permutations could not be told apart. D592 remains open.

## Why revert rather than keep

The fallback is inert by default - `answer_resolve` writes nothing unless `ORBISTOUN_APR_ANSWER` is
set, so a normal run is unchanged - which means it only ever served the experiment. That experiment was
negative on both counts. Keeping experiment-only code that softens a documented stance (D587) and rests
on an unmeasured slot assignment (D592), for no title moved and no measurement retired, is the shortcut
principle 11 rejects and the guess principle 3 refuses. So it is gone, and this entry is the record that
the fallback path was tried and why it is not there - so the next reader does not rebuild it.

## What remains true

Terminator's real wall is the `int 0x41` guest assert, preceded by error-message formatting (a 776-byte
`strlen`), cause not yet identified - it needs its own investigation (what sets the error flag the
assert tests, `test byte [rax+0x30],0x10; je; int 0x41`), independent of APR. The APR out-param slot
layout (D592) is still the honest report-not-guess it was before this.

## Corrected 2026-09-25: the trap is APR's, and this experiment could not have shown it

Worklog 867 traced the `int 0x41` return address by return address. A stream's length method
returns the unfilled size slot of this resolve, which holds a leftover stack address. The title then
reserves a string that long (`sceKernelReserveVirtualRange`, `0x600000800000` bytes), is refused, and
traps. So the resolve **is** in the fault chain.

The negative result above is explained by how it wrote: `answer_resolve` stores **four bytes** into
each slot. A 64-bit size slot keeps the upper half of the old stack address, so the "answered" size
was still about `0x6000_0000_000d`, and all six permutations failed identically. The decision not to
ship a guessed layout stands. The layout is now requested from hardware (REQ-20260925T1834Z-a7e2).
