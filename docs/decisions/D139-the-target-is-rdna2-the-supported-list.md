# D139 - The target is RDNA2; the supported list names instructions rather than numbering them


**Status:** decided (target confirmed)

The target console's GPU is RDNA2-derived. Everything in the shader subsystem was built
against `gfx900` - the fifth generation of the architecture - which is the wrong one, and
nobody had checked. The previous console generation is a "nice to have", and is GCN, so
the existing tables are close to *that* target rather than wasted.

Two generations, then, and that settles a design question that would otherwise have been
a judgement call.

**`model::SUPPORTED` is a list of names.** It was `(family, opcode)` pairs. Opcode numbers
belong to one generation and most of them move between generations - the same arithmetic
at a different number, in a family whose identifying bits also changed. Measured: 52% of
encodings differ between the two, and 21% of the probe set does not assemble on the newer
one at all.

A list of numbers pointed at another generation **does not fail**. It binds silently to
whichever instructions occupy those numbers, and the first sign is a wrong pixel. Names
mostly survive; the handful that do not - one generation's `v_add_u32` is another's
`v_add_nc_u32` - arrive as a list of exactly what needs attention, which
`every_supported_name_exists_on_this_target` produces. Verified by renaming one and
confirming it is named.

**Names come from two generated sources, merged with a conflict check.** The probe solver
names every opcode it solves per opcode; instructions whose operand shape comes from their
family's layout are not in that set, and the fixture generator covers most of the gap.
Both run the same reference assembler against the same target, so a disagreement means a
generator has drifted and is a load failure rather than a silent preference.

**Now done:** the dispatch matches on names internally too. That is the half that makes a
retarget *cheap*; the list above is the half that makes it *loud*, and loud mattered more,
so it landed first. Six internal `(family, opcode)` matches, two width tables and a
sub-encoding list are names now, and the name is resolved once at the top of
`translate_instruction` - an opcode with no recorded name is refused there rather than
dispatched on, which is the point.

Two things the conversion turned up that the numbering had hidden:

- The multi-word accesses derived their width from **opcode arithmetic** - `1 << opcode`
  for the scalar loads, a lookup table for the flat ones. Both encode an ordering that
  holds on one generation by coincidence. Reading the count out of the `dwordxN` suffix
  is the instruction's own statement of its width and needs no table at all.
- `resolve` - the auto-fidelity decision - was passing a *family* where a name was
  wanted, and it compiled, because both are `&str`. Two tests caught it, and only because
  they assert on behaviour that depends on the answer. Worth noting as the cost of
  keying on strings: the compiler stops helping. The type would have caught it; the tests
  did instead.

