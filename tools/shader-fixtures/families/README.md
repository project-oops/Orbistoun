# Family probes

One assembly file per instruction encoding family. `orbistoun-gen encodings` reads them and
solves each family's identifying bits, opcode position and instruction width from the assembled
bytes.

Only membership is declared here: which instructions belong to `VOP3` comes from a person reading
the published instruction-set reference. Every number - the mask, the value, where the opcode
sits, how wide the instruction is - is derived from what the assembler emits, never transcribed.

## Probe file rules

The solver depends on two properties it cannot supply itself:

1. **At least three mnemonics**, spread across the family's opcode range. The opcode field is
   found as the bits that differ between mnemonics; two adjacent opcodes differ in one bit and
   would solve a one-bit field.
2. **At least three operand variants of each mnemonic**, using different and high register
   numbers. Operand bits also differ between mnemonics, and what separates them from opcode bits
   is that they also differ within a mnemonic. A mnemonic that appears once contributes no such
   evidence and corrupts the solve.

A file that breaks either rule does not produce a wrong answer: the solver refuses with
`opcode field could not be solved`, because the leftover bits are not contiguous.
