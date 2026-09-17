# 623. The four libraries in the stall finding are called zero times, and one excuse had rotted

**2026-09-16** - orbistoun-service, the finding filed as `REQ-20260915T0929Z-2f2f`

The finding says four declared-but-empty libraries "sit on the boot path" and "fail as stalls, not
faults" - `libSceAvPlayer` (22 names), `libSceAjm` (14), `libSceSaveData_native` (5) and
`libSceMsgDialog.native` (4). Before implementing anything into them, the premise was checked
against the run data. **None of the four is called once, by any guest, in any recorded run.**

## The measurement

`orbistoun-cli worklist --top 400` ranks every import by how often a guest called it, across all six
title runs. Its floor is one call, so a name absent from it was never called. Cross-referencing the
23 libraries in `SERVES_NOTHING` against it:

| Library | calls | functions reached |
|---|---|---|
| libSceKeyboard | 283 | 3 of 3 |
| libSceNetCtl | 194 | 2 of 2 |
| libSceVideoRecording | 16 | 4 of 4 |
| libSceAmpr | 3 | 3 of 5 |
| libSceCoredump | 1 | 1 of 1 |
| libSceErrorDialog | 1 | 1 of 1 |
| the other 17, including all four in the finding | 0 | none |

The worklist does count calls into libraries that implement nothing - `libSceKeyboard` is in
`SERVES_NOTHING` and its three names are right there with 283 calls between them - so the zeroes are
zeroes and not an artefact of the tool only counting implemented functions. That check matters: had
the worklist listed only implemented imports, every number above would have been meaningless in the
same direction.

## What that does to the finding

The four named libraries are imported, resolvable and never invoked. Their declarations are doing
exactly the job D505 gave them - a guest that reaches one gets named and counted instead of dying on
an unresolved import - and nothing has reached one.

So the "stalls, not faults" reasoning is not observable anywhere. The two furthest titles end on a
*fault* (`VCRUNTIME140.dll+0x1dc8d`, the AGC patch wall); the ones that end on the clock do not call
these libraries at all. No run stalls on a declared-empty library, because none of the six libraries
that *are* reached is called often enough to be a spin - the largest is `sceKeyboardReadState` at 263
calls in a run of 418,000, which is 0.06% and a poll that returns, not a loop that waits.

Implementing any of the four now would be writing code no run can exercise, which is the thing
principle 6 exists to stop. The finding should be re-aimed at the six libraries a guest does reach,
and re-examined once something gets past the AGC wall - that is what would first put a title in
front of a movie player or a save dialog.

## The rot this turned up

`SERVES_NOTHING` entries open with `declared as N name(s)`, and nothing checked N.
**`libSceVideoRecording` claimed ten; its module declares four.** Six names had been taken out of
that module for having no provenance - they came from a probe check that was read as showing they
*resolve* and in fact records the branch where the lookup returned null - and the excuse was not
updated with the removal.

What makes it a good example rather than a typo is that **two other numbers describe this same set
and neither was wrong.** The existing guard checks membership, not size. README's `149 across 23
libraries` is counted from the declarations in `orbistoun-cli` and never reads this string - and
149 is right, while the excuses sum to 155. A transcribed number flanked by two correct derived ones
is the easiest kind to leave rotting, because nothing that is checked disagrees with it.

Fixed, and now guarded: `an_excuse_that_states_a_name_count_states_the_right_one` parses the count
out of each reason and compares it with `module.imports.len()`. It is tolerant of a reason that does
not open that way, since a bespoke reason has nothing to check - and because that tolerance is what
would let it check nothing at all, it also asserts it checked all 23.

Both directions were made to fail: restoring the ten produced
`libSceVideoRecording says it declares 10 name(s) and its module declares 4`, and changing the prefix
it parses produced `left: 0, right: 23` from the count assertion.

## Files

- `crates/orbistoun-service/src/symbols.rs` - the corrected excuse and the new guard.

## Next

`2f2f` is not implementable as written and is annotated with the measurement rather than closed -
the six reached libraries are the version of it worth doing, and none of them is urgent. Still open
in this lane and blocked on obSCEne: the packed-format measurements (`REQ-20260916T1250Z-b3d4`) and
the draw capture four roadmap items sit behind (`REQ-20260916T1250Z-a1f7`).
