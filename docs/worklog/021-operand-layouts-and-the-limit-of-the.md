# 2026-08-19 - Operand layouts, and the limit of the per-family model


**Done.** Layouts for every family the model can actually describe, plus the two
decoder capabilities they needed: an operand may name a later dword, and a field may
store a scaled register index.

`Encoding::operands` became `Option<Vec<_>>`, so "no layout established" and "checked,
takes no register operands" are different states rather than the same empty list.

**Verified.** Gate green. **Seven families of seventeen** carry a layout, and all 99
operands they produce match the reference disassembler across the fixture corpus.

**Surprises.**

- **The premise of the task was wrong, and the test proved it in three rounds.**
  Layouts were written for all seventeen families; ten were rejected. Operand layout is
  a property of the *opcode*, not of the encoding family - the count varies within a
  family, and in the memory families the field *selection* varies too, so a load writes
  a destination where a store reads data, from different bits. Truncating a fixed list
  fixes the first and not the second. (D096)

- **Each rejection was a different bug wearing the same clothes.** A scalar destination
  using the source numbering; a source modifier printed as a sign; a three-source family
  with two sub-layouts. All three produce output that looks entirely reasonable, and all
  three were found in seconds by a test that cost an afternoon.

- **Two mechanical edits missed because `cargo fmt` had reflowed the code between
  writing the replacement and running it.** The fifth and sixth overreach of the
  session, by a new mechanism - the text matched when written and not when applied.

- **An insertion loop assumed every entry had a blank line after it.** The last entry in
  the file does not, so nothing was written at all. Harmless because the write came
  after the loop; it would not have been if it were inside.

**Not done.** Ten families still have no operand layout, deliberately. Per-opcode
operand data - derivable from the reference the way mnemonics already are - is the next
mechanism and has not been built. Nothing translates.

