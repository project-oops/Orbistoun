# D265 - The model gets its own binary, not a place in the CLI


**decided** · 2026-08-25 · from a stated constraint, and the measurements agree with it

The brief: a model belongs only where it genuinely helps, cannot produce a negative result,
and cannot block the person using the tool. That resolves the open question about where the
loop should live.

**One principle covers it.** A model earns its place where the candidate space cannot be
enumerated *and* a proposal can be checked mechanically. Both halves are load-bearing. Where
enumeration works it wins - measured repeatedly: a boot costs about 0.13 seconds against
five to twenty for an answer, so every sweep in this project beats a model asked to choose.
Where there is no oracle, the output is plausible and unverifiable, which is the failure
this repository takes most seriously.

Naming vocabulary is the only place in orbistoun that satisfies both. You cannot loop over
every plausible English noun; and the NID hash decides each proposal for free, so a wrong
one costs a sweep and vanishes.

So `orbistoun-suggest` is a **separate binary** in `orbistoun-propose`, reached by
`./orbistoun.sh suggest`. `orbistoun-cli` gains no dependency on `orbistoun-llm`, which
matters because the CLI is what `./orbistoun.sh run` calls and that command has to stay
fast. The run report *mentions* the tool in the action on an unnamed import, and never
invokes it.

**Ruled out, each for a stated reason:** writing implementations (no oracle); proposing stub
return values (has a one-bit oracle, but twenty-three imports sweep exhaustively in six
seconds); choosing the next experiment (D231); anything inside `run` (blocks); mutation and
combination of existing words (a loop does both exhaustively in milliseconds).

