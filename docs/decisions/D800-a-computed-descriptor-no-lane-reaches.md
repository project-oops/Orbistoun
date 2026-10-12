# D800 - a computed descriptor no lane reaches is null

**Status:** decided
**Date:** 2026-10-12

The host replay of a program's prefix (D753) starts from the system registers the module seeds for
the draw, as well as its user data: for a primitive shader, gs_tg_info in `s2` and merged_wave_info
in `s3`, laid out as the module lays them (`si_shader_args.c:304-322`), from the draw's vertex count
and the primitives its assembly makes of them. The wave's execution mask, which the program derives
from `s3`, is then known, and so is any branch on a mask copied from it.

Where a branch the replay follows jumps past the access, the wave does not reach it. A descriptor it
would read through is null: its range binds nothing and its fourth word is zero, which a format
load reads as zeros under every select (`null_format_load`, radv's null buffer).

Assumed (`known_by = assumed`): that a forward jump past the access is not undone by a later branch
back to before it. The prefix ends at the access, so the replay cannot see such a back edge; ACO lays
structured control flow out forwards, and a loop around the access would put the branch inside it,
where the next pass decides it again from the same uniform values.

**Why:** PPSA02664's vertex shader computes a descriptor it reads through at 0x488, after
`s_cbranch_vccnz` at 0x3a4 on a mask that is `exec` wherever its uniform compare holds. The replay
did not know `exec`, so the draw's buffer formats were refused and the draw was not carried out.

**Rejected:**
- Seeding the execution mask at entry as every lane of the wave, as the lane run does (D758): the
  program sets `exec` from `s3` before it reads it, so the seed that matters is `s3`.
- Refusing an unreached access as before: no lane reads through it, and refusing it refuses a draw
  that is fully defined.
