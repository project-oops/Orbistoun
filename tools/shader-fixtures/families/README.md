# Family probes

One file per encoding family. `orbistoun-gen encodings` reads them and solves that
family's identifying bits, opcode position and instruction width from the assembled
bytes.

**Membership is the only thing declared here.** Which instructions belong to `VOP3` comes
from a person reading the published instruction-set reference. Every *number* -
the mask, the value, where the opcode sits, how wide it is - is derived from what the
assembler emits, never transcribed. That split is the point: see D085, and D139 for what
transcribing them cost.

## What a good probe file looks like

Two rules, both of which the solver depends on and neither of which it can supply:

1. **At least three mnemonics**, spread across the family's opcode range. The opcode
   field is found as the bits that differ *between* mnemonics; two adjacent opcodes
   differ in one bit and would solve a one-bit field.
2. **At least three operand variants of each mnemonic**, using different and high
   register numbers. Operand bits also differ between mnemonics, and the only thing
   separating them from opcode bits is that they *also* differ within a mnemonic. A
   mnemonic appearing once contributes no such evidence and poisons the solve.

The failure mode of getting this wrong is not a wrong answer - it is `opcode field could
not be solved`, because the leftover bits are not contiguous. That refusal is deliberate.
