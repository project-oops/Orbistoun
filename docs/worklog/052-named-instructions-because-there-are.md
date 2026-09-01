# Named instructions, because there are two targets


The target is confirmed as the RDNA2-derived part, with the previous console generation a
nice-to-have. So the shader subsystem needs to serve two architecture generations, not
one - which turns "should translation key on opcode numbers or names" from a judgement
call into a settled question.

`model::SUPPORTED` is now sixty-seven **names**. `supports_named` resolves them through
whichever tables are loaded, and `unresolved` reports any the loaded generation does not
have. `every_supported_name_exists_on_this_target` fails with that list.

The argument is not that names mean fewer edits. It is that **a list of opcode numbers
pointed at another generation does not fail** - it binds silently to whatever occupies
those numbers, and the first sign is a wrong pixel. A list of names either resolves or
says which name is gone. Verified by renaming one instruction in the tables and confirming
the test names it and nothing else.

**Surprises.**

- **Six supported instructions had no recorded name**, because their operand shape came
  from their *family's* layout rather than a per-opcode one, so the probe solver had no
  entry for them. Invisible until dispatch depended on names. Fixed by probing them, and
  by merging the fixture generator's names as a second source with a conflict check -
  both are generated from the same assembler against the same target, so a disagreement
  means a generator has drifted and deserves a load failure.

- **The two-sample rule blocked `s_endpgm`.** It takes no operands, so one sample was all
  the probe file had. The rule exists so a *field* is not inferred from one observation,
  and an entry with no fields has nothing to infer - relaxed narrowly rather than padding
  the probe file with duplicate lines, which would have satisfied the letter of the rule
  and none of its purpose (D140).

- **The existing gfx900 work is not wasted.** GCN5 shares most of its instruction set with
  the previous console's generation, so what looked like building against the wrong target
  turns out to be most of the way to the optional one.

**Not done.** The dispatch still matches on `(family, opcode)` internally - eighteen arms
and fourteen inner comparisons. That is the half that makes a retarget cheap; this was the
half that makes it loud, and loud matters more. The regeneration against the real target
waits on that conversion, because doing it first would break sixty places at once with
nothing to guide the repair.

