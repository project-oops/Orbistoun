# 2026-08-19 - Operands, and a surface to see them through


**Done.**

- `orbistoun-shader::operand`: the unified operand numbering in `data/operands.toml`,
  per-family layouts in `data/encodings.toml`, decoded into `Instruction::operands`.
- Differential verification of operands against the reference disassembler.
- `orbistoun-cli shaders <dir> [--top N]`, rendering through the library.

**Verified.** Gate green. 57 tests in the crate, **99 operands checked against the
reference** across the fixture corpus, and the command renders the ranked worklist over
real material.

**Surprises.**

- **The operand test found a real bug on its first run, of exactly the class it was
  built for.** A scalar *destination* field is not a plain register index - scalar
  registers stop at 101 and the codes above name the special registers. We were
  reporting `s106` where the reference said `vcc`. Nothing about that output looks
  wrong: scalar register 106 exists, the instruction decodes cleanly, and a translator
  built on it would emit a shader that compiles and draws the wrong thing. (D094)

- **Two mechanical edits overreached again, in one session.** A `use` replacement
  matched the import inside `mod tests` as well as the module-level one, and appending a
  function to the end of a file put it after the test module. Both were caught by the
  compiler in seconds, which is the point - but that is now four times a
  scope-by-text-match edit has gone wide. The rule that keeps needing relearning: scope
  a mechanical transform by what it *means*, not by what it *matches*.

- **The new CLI command reproduced the project's own cautionary tale.** Pointed at the
  fixtures directory it read the reference-text files as shaders and reported eighteen
  where there are nine. A tool built to catch plausible-but-wrong output produced
  plausible-but-wrong output on its first run.

**Not done.** No translation - the decoder now knows what instructions operate on, and
nothing emits SPIR-V. Operand layouts exist for six families of seventeen; the rest
report `operands_decoded = false` rather than an empty list, so they cannot be mistaken
for complete. Multi-register operand *width* is not decoded, which is why the
differential test collapses `s[4:5]` to `s4` and accepts `vcc` for `vcc_lo`.

