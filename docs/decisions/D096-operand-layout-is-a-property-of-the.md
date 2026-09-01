# D096 - Operand layout is a property of the opcode, not of the encoding family

**decided** · 2026-08-19

Recorded because it invalidated the premise of a piece of work, and the premise looked
obviously right.

The plan was to declare an operand layout for each of the seventeen encoding families,
the way each already declares an opcode position and a width. Layouts were written for
all seventeen. The differential test rejected ten of them, in two distinct ways:

- **The count varies.** `v_mul_f32_e64 v3, s7, s7` and `v_fma_f32 v4, -v1, v3, 1.0`
  share an encoding family and take two and three sources. A fixed list decodes fields
  that are not operands and reports them confidently.
- **The field *selection* varies.** In the memory families a load writes a destination
  register where a store reads a data register, from different bits. Truncating a fixed
  list to the right length still picks the wrong field, which is worse than picking too
  many - and `v_div_scale_f32 v1, vcc, v0, v0, v0` shows the same split inside the
  three-operand vector family, which has two sub-layouts distinguished by opcode.

So a per-family layout can only describe a family whose shape is fixed. **Seven
qualify** - the scalar and vector ALU families, plus the branch family which was
checked and genuinely carries no register operand.

The other ten are left with no layout at all rather than an approximate one. That is
the whole point of `operands_decoded`: an instruction reporting no operands because
nobody has taught the decoder its family is a different claim from one that takes none,
and a translator must be able to tell them apart.

**What it would take.** Per-opcode operand data, derived from observation the way the
mnemonic table already is (D090) - the reference prints the operands, so their number
and kind can be read off rather than transcribed. That is a different mechanism from
the per-family table and is left for its own piece of work.

**Two smaller findings**, both caught by the same test:

- A scalar *destination* shares the source numbering, so codes above 101 name special
  registers rather than scalar ones (D094).
- A leading sign on a register operand is a **source modifier**, a separate field. The
  operand field still encodes the plain register, so the differential test strips the
  sign before a register letter - and only there, since stripping it everywhere would
  make `-1.0` match `1.0` and hide a genuine sign error in the inline constants.

