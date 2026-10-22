# D945 - A dispatch whose buffer is not guest memory yet is polled

**Status:** decided
**Date:** 2026-10-21
**known_by:** assumed

A compute dispatch may read its buffer descriptors out of a table in memory, and a run that reads the
table before the title has written it reads whatever was there: a descriptor naming no guest memory,
or one whose out-of-bounds mode bounds nothing. A refused dispatch is therefore not refused at once.
The queue stands at it as it stands at a memory wait (D932): the command processor prepares it again
each time it polls, every millisecond, and goes on once it runs. The streams behind it run after it
in order. A dispatch still refused five seconds after its first refusal is refused, as before.

**Why:** Grand Theft Auto V's dispatches at bytes 0x10edc and 0x185fc read their index and vertex
buffer descriptors from per-draw tables (`s_buffer_load_dwordx8 s28, s0, 0x8`, offset by
`s7 * 0x78`). The same build ran each in one run and refused it in another, finding `0x10a9aaff4c`
outside every guest region, or a zero descriptor; 77 submissions waited behind the refusal, among
them the release three compute streams wait for.
