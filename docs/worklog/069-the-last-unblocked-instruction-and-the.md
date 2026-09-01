# The last unblocked instruction, and the buffer descriptor's operands


`v_fmac_f32` is translated, which finishes G7's unblocked work: 110 of 127 instructions,
6 of 10 fixtures. The roadmap is refreshed - it still claimed the division helpers were
blocked on documentation and the subgroup level was a stub, and its counts predated the
retarget entirely.

Then the first half of G10. The published reference **does** specify the guest side of the
buffer resource model, so MUBUF's operand layouts are solved and the addressing equation
is in hand. 604 workspace tests green.

### Surprises

- **The addressing equation is a figure, and the text extraction silently dropped it.**
  `ADDR = Base + baseOffset + Inst_offset + Voffset + Stride * (Vindex + TID)`, with each
  term labelled by where it comes from. Rendering the page as an image got it. Worth
  remembering for the rest of this document: a section that reads as oddly empty in the
  text dump is a diagram, not an omission.
- **The solver could not express a buffer resource operand at all**, and said so as
  "unsolvable", which reads as a gap in the *probes*. It was three gaps in the solver:
  five-bit fields were never tried (`WIDTHS` started at six), scale four was never offered
  (a resource constant names a group of four registers, so its field holds a quarter), and
  the scale had to be added in two places because the candidate loop only matches scales
  the operand reader proposes. An unsolvable opcode is now a suspect rather than a fact.
- **Two rounds of the too-narrow-field fault, in one instruction.** The data field solved
  seven bits wide because no probe used a register above 127, and the scalar-offset field
  solved seven because no probe used an inline constant. Both borrowed the missing bit from
  the field beside them and were right for every sample given. This is the fifth and sixth
  time this project has hit that pattern.
- **The layout was solved before the document was read**, and the two agree on all four
  fields. That is the pairing that corrected the reference over the LDS opcode; this time
  it confirms it.
- **A first attempt made the destination one below the address register in every sample**,
  so the two fields were indistinguishable and the load would not solve at all. The store
  solved only because its samples happened to break the pattern.

### Outstanding

The MUBUF *translation* is not written - what exists is the operand layout and the
addressing rule. The descriptor is read from four scalar registers at runtime, so base,
stride and record count are values rather than constants, and the out-of-bounds behaviour
has its own table. That is the next unit.

