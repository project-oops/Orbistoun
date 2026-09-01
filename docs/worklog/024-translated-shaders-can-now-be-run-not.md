# 2026-08-19 - Translated shaders can now be run, not just validated


**Done.** The oracle, before the thing it will judge.

- `orbistoun-spirv` gained the vocabulary for a storage buffer - types, constants,
  decorations, access chains - and a module that writes a known value into one.
- `orbistoun-gpu-vulkan::compute`: probe, dispatch, read back. Roughly forty documented
  `unsafe` blocks, which is what the workspace lints require and worth every line.
- Three tests that prove the runner against a shader whose answer is known, before it
  is trusted with anything translated.
- The gate reports whether the device tests actually ran.

**Verified.** Gate green. Both emitted modules validate under `spirv-val`, and all three
dispatch tests pass **on a real device** - an RTX 5070 Ti on the development machine,
with lavapipe available in the VM for a deterministic second opinion.

**Surprises.**

- **The loud skip was silent.** A test harness captures the output of a *passing* test,
  so the carefully written "this did not run" banner appeared nowhere. The design was
  right and the implementation defeated it, which is worse than not having tried -
  anyone reading a green suite would have believed it. Fixed in the gate, which re-runs
  those tests with output shown and says which happened.

- **`ash` was pinned to `linked`.** Nothing used it, so nobody had noticed that it
  makes the *build* require a Vulkan SDK. `loaded` defers that to runtime, which is
  also what lets a machine with no Vulkan compile and skip rather than fail to link.

- **Every SPIR-V opcode number written from memory was correct**, confirmed by
  `spirv-val` on the first run of each module. Pleasant, and not something to rely on -
  the reason it is known at all is that a validator was wired up before anything was
  built on top.

- **`too_many_lines` was right.** Two extractions came out of it - buffer creation and
  pipeline creation - and both are genuinely cohesive units that read better than the
  straight line they came from.

**Not done.** The translator still translates nothing. The runner exists to judge it and
has judged only a hand-written shader so far.

