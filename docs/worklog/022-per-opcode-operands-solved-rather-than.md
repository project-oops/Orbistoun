# 2026-08-19 - Per-opcode operands, solved rather than written


**Done.** The mechanism D096 said was needed.

- `orbistoun-gen operands`: assembles probes with `llvm-mc`, solves each
  operand's bit field by correlation over the samples, emits
  `data/opcode-operands.toml`.
- `EncodingTable` loads and prefers the per-opcode layouts; `SlotKind::Immediate` added
  so offsets decode as values rather than being skipped.
- Probe assembly under `tools/shader-fixtures/probes/`.

**Verified.** Gate green. **10 opcodes solved, 142 operands checked against the
reference** (was 99), covering VOP3, SMEM and DS - three families that had no layout at
all. One opcode reports as unsolved rather than approximated.

**Surprises.** Four distinct ambiguities, each found by the differential test rather
than by thinking, and each producing output that looked entirely reasonable:

- **Scalar registers stop at 101**, so a seven-bit field explains every register sample
  a real eight-bit field does. The solver picked the narrow one and read `exec_lo` for
  the literal marker. Only inline constants and special registers reach high enough to
  pin the width.
- **Consecutive registers are the worst possible samples.** With `v5, v6, v7, v8` in
  adjacent fields, a field shifted slightly reads values differing by the same constant
  and looks consistent across every sample. Spread and disorder are what kill that.
- **A position that only ever holds a register cannot be told from the shared
  numbering.** Code 242 is register 242 or the constant `1.0`, and it decoded `v242`
  where a shader meant `1.0`. Now refused as ambiguous rather than picked - the fix is a
  better probe, not a tie-break.
- **An operand that is not a register is still an operand.** Refusing to solve a whole
  opcode because one position held an offset lost three of eleven; immediates are fields
  too, and solvable the same way once the probes vary them.

The pattern across all four: **a solver is only as honest as its samples**, and the
failure mode is never an error - it is a confident answer that happens to be wrong.

**Not done.** Ten opcodes is a start against a few hundred. The probe set covers what
the fixtures already exercise; widening it is mechanical and unbounded. Nothing
translates.

