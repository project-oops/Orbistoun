# 519. The APR slot-layout experiment, and Terminator's real wall (int 0x41, not APR)

**2026-09-12** - operator: "yes, run the Terminator APR experiment"

Ran the experiment worklog 517/518 proposed for PPSA25872 (Terminator 2D): an on-disk fallback for the
APR index, plus a sweep of the six `ORBISTOUN_APR_ANSWER` out-parameter permutations against the guest
oracle. It gave a clean negative result and corrected an earlier misattribution.

## Result

- **The fallback works.** With it, `/app0/Media/RuntimeInitializeOnLoads.json` (present on disk, 13
  bytes; Terminator ships no `ampr_emu.index`) resolved, and `sceKernelAprResolveFilepathsToIdsAndFileSizes`
  returned `0x0` instead of the placeholder.
- **Terminator did not move, on any permutation.** All six (23/24/32/34/42/43) still fault at the same
  `int 0x41` at `image+0x17554a3`, ~321,96x calls, verdict same/BACK. Decisively: the APR-resolve call
  site (`0x3aa49e`) is **not in the fault chain** (`0x1755b55 <- 0x7b6081 <- ...`). So the `int 0x41`
  assert is unrelated to APR - **worklog 517/518's attribution of Terminator's wall to the APR resolve
  was wrong**; the placeholder return was incidental.
- **The slot layout (D592) stayed unmeasured.** Terminator provides no oracle - the "Unknown error
  occurred while loading" message D592 used for PPSA02664 never appears, and the assert fires regardless
  of the permutation - so the six could not be distinguished. D592 remains open.

## Decision

Reverted the fallback (D679). It is inert without `ORBISTOUN_APR_ANSWER` (a normal run is unchanged), so
it only ever served this experiment, which was negative on both counts. Keeping experiment-only code
that softens D587 and rests on an unmeasured slot assignment (D592), for no title moved, is the shortcut
principle 11 rejects. Tree is back to the committed APR behaviour; suite green.

## Terminator's real wall

`int 0x41` at `image+0x17554a3`, guarded by `test byte [rax+0x30], 0x10; je +2; int 0x41` - a
conditional guest assert that fires when an error flag (bit 4 of `[rax+0x30]`) is set. It is preceded by
error-message formatting (a 776-byte `strlen`, `memcpy`). So the guest hits an error, formats a message,
and traps - and the error is *not* the APR resolve. What sets that flag is unidentified and is the next
Terminator investigation: disassemble around `image+0x1755xxx` for the branch that sets bit 4 of
`[rax+0x30]`, or capture the 776-byte message the guest is formatting. Left for a later turn.

## The retail frontier, corrected

- **Earthion / PPSA02664 / Summer Sports / ASTRO BOT**: the Phase-6 AGC->Vulkan GPU engine (worklogs 516-518).
- **GTA V**: the direct-memory allocator refusal (worklog 518; handed off as a task) - a plain bug.
- **Terminator**: an unidentified guest assert (`int 0x41`), *not* APR - needs its own trace.

So of the five, GTA V remains the one wall that is a discrete, non-GPU bug; Terminator is now back to
"cause unknown" rather than the APR gap it was mistaken for.
