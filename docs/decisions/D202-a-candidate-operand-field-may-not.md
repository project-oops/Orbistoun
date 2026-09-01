# D202 - A candidate operand field may not overlap anything the encoding table already reads


**Status:** decided (2026-08-21)

D109 discards a candidate operand field that overlaps the family's **opcode** bits. The
rule is right and it was stated too narrowly: the opcode is not the only part of the first
word already spoken for. The family's own **mask** is fixed by definition - those bits are
what identify the family, they are constant across every instruction in it, and no operand
can vary there either.

So the exclusion is a mask of everything the table already reads, and D109 becomes the
case of it that happened to be found first.

**Keyed by word, not one mask.** This entry first said "a single bitmask, `mask | opcode`",
which was true for about an hour - until D105's split opcode landed a fourth opcode bit in
the *second* word, where a first-word mask cannot reach it. The exclusion is
`{word: bits}`, and the general statement in this entry's title is the one to hold: not
"the first word's fixed bits", but *anything the encoding table already reads*.

### What the missing half cost

`v_mov_b32_e32` and `v_rcp_f32_e32` had **no operand row at all**, which is to say a move
decoded with an empty operand list.

VOP1 on this target is `[31:25]` family, `[24:17]` destination, `[16:9]` opcode, `[8:0]`
source. The destination is an eight-bit vector register at bit 17. A *nine*-bit window at
the same position reaches bit 25 - the low bit of VOP1's `0xFE000000` mask, permanently 1 -
so it reads `v0` as 256, and 256 is exactly `v0`'s code in the shared source numbering.

```
v_mov_b32_e32 v0,   v1     ->  0x7e000301
v_mov_b32_e32 v255, s101   ->  0x7ffe0265
v_mov_b32_e32 v100, -1     ->  0x7ec802c1
                                    ^ dst reads as vgpr 0/255/100, or source 256/511/356
```

Both readings explain every sample and neither can ever be eliminated, because the bit
that separates them is a constant. The solver did the right thing and refused. It was
right in a way nobody could see.

**The same shape as D109's own example**, which is the part worth noticing. D109 was found
because `v_cmp_lt_f32_e32` was unsolvable while its sibling `v_cmp_eq_f32_e32` solved - a
difference in one opcode bit. Here there is no sibling to compare against, because *every*
VOP1 instruction has the same family bits. A fault with no asymmetry to expose it.

### Verified the same way D109 was

Re-solving moved nothing: **zero rows removed, fourteen added**, purely additive. 68 of 71
probed opcodes solved before, 70 after.

### The reason it went unnoticed for so long

`every_decoded_operand_appears_in_the_reference` iterates the operands the decoder
produced and looks each one up in the reference disassembly. Decode none and the loop body
never executes, so it passes - and the differential suite's whole point is catching exactly
this class of error.

A one-directional check cannot see an empty answer. So there is now a converse:
`an_instruction_that_decodes_no_operands_is_a_listed_gap` asserts the set of instructions
decoding nothing is **exactly** a written-down list, each entry with its reason. Closing a
gap fails the test until the entry is deleted; opening one fails it until the entry is
added with a justification.

It started at twelve entries and is at **seven**: EXP and VINTRP's three have no layout
established, MIMG's one needs a resource layout the guest side cannot supply, and
`s_waitcnt`/`s_clause` are structured immediates. The five typed-buffer entries came off it
the same day, which is the test working - the list shrinking is a thing somebody had to do
deliberately, and it fails loudly if the list and the code disagree in either direction.

That is the durable half of this decision. The overlap rule fixes two opcodes; the converse
test is what stops the next silent empty answer being found by accident.

