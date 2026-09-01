# D108 - An operand the encoding does not carry is recorded, not omitted


**Status:** decided (2026-08-21) - the discrimination is measured now, not asserted

`SlotKind::Implicit` describes an operand that occupies no bits. The 32-bit comparison
forms write the condition mask and nothing else, so `vcc` is printed by the reference and
encoded nowhere.

Leaving it out of the layout was the alternative, and it would have made a decoded
comparison not mention the register it writes - which is the operand that matters most
about it. A report reading `v_cmp_lt_f32 v0, v1` hides the whole effect.

**The claim is evidenced rather than assumed.** Two things look identical to a solver: an
operand the encoding does not carry, and one the probes never varied. It emits
`implicit` only when no field anywhere explains the operand *and* the text is identical
in every sample. The 64-bit form of the same comparison does encode a destination, and
does solve one.

### The part of that which was prose, and now is not

The original entry justified the discrimination by saying the assembler refuses any other
value in that position for these opcodes. That was never checked. It was an assertion of
exactly the kind D128's width reconciliation made - and D128 turned out to be **wrong**,
in the direction that sends someone to fix a probe that cannot be fixed.

So it is asked rather than asserted, by the same technique: substitute a different value
into the operand, assemble, and compare the encoded words. Three outcomes, and the third
is the one the prose did not allow for.

| assembler says | reading |
|---|---|
| refused | nothing else is legal there. The operand is fixed. |
| accepted, words **identical** | it carries no bits, because changing it changed no bit. |
| accepted, words **differ** | there **is** a field, and the probes missed it. |

The third case is why this is worth the round trip. Recording it as implicit would file an
operand as un-encoded while the encoding carries it, and every decode of that opcode would
print the sample's value in place of the real one - a wrong answer that looks like a
right one, which is the failure this project's third principle exists to prevent. The
solver now refuses to solve rather than guessing, which is the same response it already
gives to every other unexplained operand.

**Measured, and the strongest branch is the one that fired.** For the four comparison
opcodes that carry an implicit `vcc`:

```
v_cmp_lt_f32_e32 vcc,      v0, v1  ->  0x7c020300
v_cmp_lt_f32_e32 exec,     v0, v1  ->  refused
v_cmp_lt_f32_e32 s[0:1],   v0, v1  ->  refused
v_cmp_lt_f32_e32 s[10:11], v0, v1  ->  refused
v_cmp_lt_f32_e32 vcc_lo,   v0, v1  ->  0x7c020300   <- accepted, bit-identical
```

Not merely "nothing else is legal", which would leave open that the field exists and has
one legal value. A *different spelling was accepted and produced the same word*, so the
operand demonstrably occupies no bits. The four rows in `opcode-operands.toml` survive the
check unchanged, which is the outcome that was expected - the point is that it is now an
outcome rather than an expectation.

It also settles the two-spellings note below from the other direction: the assembler
accepts both `vcc` and `vcc_lo` for the same un-encoded operand, so the disagreement
really is about naming and not about which register is meant.

**The same register arrives under two spellings.** A source field holding code 106
decodes through the operand table, which names codes as 32-bit registers, so it reads
`vcc_lo`. The implicit destination carries the text the reference printed, which is
`vcc`. Neither is wrong and neither carries the width - the width comes from the opcode -
so `model::lane_mask_name` normalises at the point of use rather than either table being
rewritten to agree with the other.

