# D129 - Side effects on hidden state are part of an instruction, not an extra


**Status:** decided (2026-08-21) - and partly verified, by a route this entry said did not exist

Every scalar instruction that writes the condition code now writes it: the 64-bit logic
(`s_and_b64` and friends), the 32-bit arithmetic and logic, and the compact forms. What
it means differs by family - the logical operations set it to whether the result is
non-zero, the arithmetic ones to whether the *signed* addition overflowed, and
`s_mulk_i32` leaves it alone entirely.

**The 64-bit logic was already translated and already wrong.** It wrote its destination
and dropped the code, so a shader doing `s_and_b64 exec, exec, vcc` and then branching on
the code read whatever the *previous* compare had left. That is not an obscure corner -
it is how a compiler skips a block once no lane survives.

Nothing in this project can observe the omission. The behaviour is documented in the
published instruction set and invisible in the encoding, the operand layout, and every
test that only checks destinations. **An instruction's hidden side effects have to be
looked up, because no oracle available here will volunteer them** - which is a different
kind of gap from the ones the differential test and the solver find, and needs a
different habit to catch.

The regression test was checked by removing the fix and confirming it fails, rather than
by assuming a passing test proves anything about the thing it names.

### There is an oracle after all, and it was already in the fixtures

*"Nothing in this project can observe the omission"* was wrong, and the counter-example was
sitting in `control.txt` the whole time:

```
0x18  s_cmp_lt_i32 s2, 1     <- sets the condition code
0x1c  s_mov_b64 s[2:3], -1   <- between them
0x20  s_cbranch_scc1 3       <- branches on it
```

A compiler that places an instruction between one setting the condition code and one
branching on it is asserting that the instruction in the middle **does not write it**. If
it did, the shader the compiler emitted would be wrong. That is an observation about real
compiled output, not a restatement of the document - the same category of oracle as using
a disassembler for encodings, and available all along.

`the_corpus_agrees_about_hidden_side_effects` mines every fixture for those windows and
checks each one against what this translator believes. Today it finds one window and
confirms one instruction: `s_mov_b64` does not write the condition code, and this
translator agrees.

**One instruction is thin and the entry should say so.** What changes is not the amount of
evidence but its availability: there is now a mechanism that turns compiled shaders into
claims about hidden state automatically, and it grows with the corpus rather than needing
anyone to remember. This entry said the class needed a different habit to catch; that habit
is now a test.

**The list stays a list, and that is not the D122 mistake.** That one duplicated something
the probe solver already recorded, so it could only drift. This cannot be derived from
anything here - a side effect on hidden state is invisible in the encoding, the operand
layout and any test that checks destinations. It has one source, so a list is the honest
shape; what it lacked was a check, and now it has one.

