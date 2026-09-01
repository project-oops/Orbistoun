# D097 - Per-opcode operand fields are solved from probes, not transcribed

**decided** · 2026-08-19

`orbistoun-gen operands` assembles hand-written probes with `llvm-mc`, which
reports the resulting bytes, and then searches for the bit fields that explain every
sample. Output is `data/opcode-operands.toml`, consulted in preference to the
per-family layout.

D096 established that most families have no fixed shape, so the fields have to come
per opcode - and transcribing several hundred of them would be several hundred chances
to be quietly wrong. This inverts it: the probe says what the operands are, the
assembler says what the bytes are, and the field is whatever is consistent with both.

**Correlation needs adversarial samples, and each kind of ambiguity had to be beaten
separately.** Every one of these was found by the differential test rather than by
reasoning, and each produced output that looked entirely reasonable:

- **Too few samples.** One sample proves nothing; any field reading the right value
  explains it.
- **Registers too low.** Scalar registers stop at 101, so a seven-bit field explains
  every register sample that a real eight-bit field does. The solver chose the narrow
  one - correct on everything it had seen, and wrong on the first instruction carrying
  a literal, where it read `exec_lo` for code 255. Beaten by probing inline constants
  and special registers, whose codes reach the top of the space.
- **Registers consecutive.** With `v5, v6, v7, v8` in adjacent fields, a field shifted
  slightly reads values differing by the same constant and looks perfectly consistent.
  Beaten by spread, non-monotonic samples.
- **No constant in a position.** A field holding 242 is vector register 242 under one
  reading and the constant `1.0` under the other, and samples that only ever put a
  register there cannot tell them apart. It decoded `v242` where a real shader meant
  `1.0`.

That last one is now **refused rather than guessed**: two readings of the same bits are
not a tie to be broken, so an operand whose kind is ambiguous leaves the whole opcode
unsolved and absent from the table. The cure is a better probe, not a coin toss.

An entry solved from fewer than two samples is rejected at load, because one sample is
not evidence.

**Result: 10 opcodes solved, covering families that had no layout at all, and 142
operands verified against the reference** - up from 99. One opcode is honestly
unsolved and therefore absent.

**Provenance.** The assembler turns source this project wrote into bytes; the
correlation is this project's own code over its own samples. Nothing is read from the
reference implementation's tables - the same line D089 draws.

