# D085 - Shader tables are data

**Status:** decided
**Date:** 2026-09-26

The instruction encodings, operand numbering, operand layouts, mnemonics and buffer formats live
in data files under `crates/orbistoun-shader/data/`. A row is measured from the reference
assembler or transcribed from the published instruction set with a citation; a mnemonic exists
only for an instruction a compiler emitted and the reference disassembler named. An instruction
with no name reports its family and opcode.

**Why:** every row is a claim about hardware, and a wrong row silently mis-decodes rather than
failing to compile. As data a correction is an edit, checkable against a real shader in seconds,
and the transcription stays separate from the code. An invented name sends a reader to the wrong
instruction; a missing one costs ten seconds.

**Rejected:**
- A `match` in Rust: a correction becomes a release.
- A full transcribed opcode table: hundreds of rows nobody can check.
