# D090 - Instruction names come only from what was observed

**assumed** · 2026-08-19

`data/mnemonics.toml` is emitted by the fixture generator, from instructions a real
compiler emitted and a real disassembler named. Thirty-seven entries.

Transcribing a full opcode table would mean hundreds of rows nobody could check, in a
project whose stated rule is that an unverified constant is worse than an absent one.
Generating it from observations inverts that: every entry is verified by construction,
and the table grows only as the fixture set grows - which also grows what the
differential test proves, so the two stay coupled in the right direction.

An instruction with no entry reports as its family and opcode. That is legible enough
to look up, and it keeps the gap visible. A missing name costs a reader ten seconds; an
invented one sends them to the wrong instruction.

The generator refuses to emit two names for one family-and-opcode. That would mean the
classification is wrong, and whichever won would be arbitrary.

