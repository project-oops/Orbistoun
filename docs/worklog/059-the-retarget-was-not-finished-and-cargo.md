# The retarget was not finished, and cargo had been hiding it


Reported the retarget green. It was not: `cargo test` stops at the first failing test
binary, `agreement` was failing ahead of `execute`, and **`execute.rs` had not run at
all**. Sixty failures were behind it.

All sixty were the same thing as the six found earlier - fifty-two encodings written down
in a test file - and all of them now go through one `head(name)` helper that asks the
table. The whole GPU side is green: 210 tests.

### Surprises

- **The lesson had a bill attached and I paid it twice.** The six tests fixed during the
  retarget were the visible ones. `execute.rs` is the largest device test file in the
  project and it never compiled its way into a run, so the same class of bug sat there
  looking fixed. *Read which binaries actually ran*, not just the last line of output.
- **`the_supported_list_and_the_translator_agree` carried its own epitaph.** Its
  hand-written encoding list had a comment saying it had already failed to catch the
  drift it exists to catch, *because the list of encodings it checks is also written by
  hand*. It is derived from the table now, and it covers every name the target has rather
  than the dozen someone thought of.
- **A supported instruction can be refused, and that is not drift.** Synthetic encodings
  with zeroed operand fields make the carry arithmetic name an ordinary register pair,
  which the translator legitimately refuses. The test asks *why* it was refused now, and
  the reason is a shared constant rather than matched prose.
- **The decoder was stopping at the wrong end of the shader.** `decode_program` stopped
  at the first `s_endpgm`, but a shader ends its wave once per exit path and the compiled
  control-flow fixture has two - so it was truncated in the middle, a branch then pointed
  at an instruction that was no longer decoded, and the report blamed the branch. What
  actually ends the code is `s_code_end` padding, which the reference describes as an
  illegal instruction placed past the end so a prefetch faults rather than executing
  memory. The decoder recognises padding now, in both modes.
- **`PROGRAM_END_OPCODE: u32 = 1`.** The same hard-coded-opcode bug, in the decoder
  itself, surviving the retarget only because `s_endpgm` happens to be opcode 1 on both
  generations. By name now.
- **`s_clause` is real and does nothing.** The reference: *define a clause of instructions
  which are executed together.* It groups the following instructions for issue and
  computes nothing, so it translates to nothing - a translation, not a shortcut, and
  cited as such.

