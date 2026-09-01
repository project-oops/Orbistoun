# D104 - A blocked instruction names its dependency; an unwritten one does not


**Status:** decided (confirmed with input, 2026-08-20)

`model::BLOCKED` carries a reason string for instructions whose semantics are understood
but which wait on a subsystem that does not exist. `exp` is the first and currently only
entry: exporting needs a render target, and every translated module today is a compute
dispatch writing to a storage buffer.

Without this, the worklist says "no translation for this instruction" about an
instruction nobody has looked at and about one blocked on a whole subsystem. Those rank
differently - the first is an afternoon and the second is not - and a list that cannot
tell them apart sends effort at whichever is most frequent rather than whichever is
next.

`exp` could have been mapped onto the observation buffer instead, which would let
fragment shaders translate end to end and be compared between fidelity levels. Rejected:
the mapping would be invented here rather than derived from anything, and a shader that
appears to work while writing its colour somewhere arbitrary is worse than one that
refuses. Principle 3.

An entry in this list is a claim that the semantics are understood and the dependency
named. It is not a to-do list - anything that could simply be written should be written,
and `s_load_dwordx2`, `s_load_dwordx4`, `s_load_dwordx8` and `s_mov_b64` were written in
the same unit rather than listed here.

