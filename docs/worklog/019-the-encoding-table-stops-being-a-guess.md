# 2026-08-19 - The encoding table stops being a guess


**Done.** Four things, in the order they unblocked each other.

- **Differential harness.** `orbistoun-gen fixtures` compiles shaders from source
  this project wrote and turns LLVM's disassembly into committed fixtures;
  `tests/differential.rs` checks our instruction boundaries against them.
- **Registers.** `orbistoun-gpu::registers` extracts register writes from a walked
  submission and reassembles shader addresses from them - the link that turns two
  disconnected walkers into one pipeline.
- **Report.** `orbistoun-shader::report` renders the summary and the ranked worklist,
  in the library so a CLI command and the run report cannot disagree.
- **Names.** `data/mnemonics.toml`, generated from what the fixtures observed.

**Verified.** Gate green. 348 workspace tests. **106 instructions across 9 fixtures,
every boundary matching the reference**, 14 of 17 encoding families covered, 37 opcodes
named. The end-to-end worklist over the fixture corpus renders correctly.

**Surprises.**

- **The contiguity check in the generator earned its place on the first run.** The
  regular expression was anchored to end of line, which silently dropped every branch
  instruction - they carry a trailing `<control+0x2c>` symbol reference. The fixture
  would have encoded a four-byte hole as real and taught the decoder that instructions
  after a branch start early. A check written on general principle caught a specific
  bug within minutes of existing.

- **LLVM crashes rather than diagnosing when a graphics shader is built for the compute
  environment.** The message *"unsupported non-compute shaders with HSA"* is there, but
  it is followed by a stack dump - and the generator was reporting the *last* stderr
  line, which is a stack frame. It sent the first investigation straight past the actual
  answer. Now it prefers a line starting with `error:`, and the triple is read from each
  source so a fixture that needs a different environment says so itself.

- **The ranked worklist validated D086 on real data without being asked to.**
  `s_mov_b32` appears 13 times but in only 2 shaders, and lands ninth. Ranking by raw
  frequency would have put it near the top and bought one shader's worth of progress.
  The judgement was made on reasoning; the fixture corpus happened to demonstrate it.

- **A mechanical `pub const` to `pub(super) const` replacement turned three public
  methods into dead code.** Scoped to a pattern instead of to the private module it was
  meant for. Third time this session a mechanical transform has overreached - the rule
  is to scope by meaning, not by text match, and it keeps needing relearning.

- **An assertion anchored on the word "shaders" selected the summary line rather than
  the first blocker**, because the summary says "shaders 0 of 9 complete" too. It had
  a third arm checking for an underscore, which every mnemonic contains, so the test
  passed while checking nothing. Two weak conditions can hide a broken one.

**Not done.** No CLI surface - `orbistoun-cli/src/main.rs` was live in another session.
Three encoding families remain unverified. The shader-address register map has no
oracle and its output is explicitly a candidate. Nothing is wired into the HLE registry,
so no real submission has been walked and no real shader captured.

