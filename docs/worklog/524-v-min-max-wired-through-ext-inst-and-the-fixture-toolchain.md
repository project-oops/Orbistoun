# 524. v_min_f32/v_max_f32 wired through the extended set, and the fixture toolchain pinned

**2026-09-12** - hardware down; advancing the GPU pipeline offline (compute oracle, no PS5)

Worklog 523 added `OpExtInstImport`/`OpExtInst` and the `GLSL.std.450` plumbing. This spends that
capability on its first callers - `v_max_f32` and `v_min_f32`, the saturating VOP2 pair a shader
reaches for constantly - and verifies them by dispatch on a real device, not by validation alone.

## What landed (orbistoun-translate)

- `Model::glsl_set()` (required) lazily imports `GLSL.std.450` and caches the id, implemented on both
  `Wavefront` (owns the builder) and `Predicated` (per-lane). `f32_ext_binary(instruction, lhs, rhs)`
  (provided) mirrors `f32_binary`: it bitcasts the two operand registers to `f32`, emits an `OpExtInst`
  into the imported set for the given instruction number, and bitcasts the result back to `u32`. A
  register holds bits, not a typed value, so this reinterprets rather than converts - the same rule the
  core-opcode float ops already follow.
- `float_min_max` dispatches `v_max_f32_e32`/`v_min_f32_e32` to `FMax` (40) / `FMin` (37) per lane.
- `SUPPORTED` gains both mnemonics so `supports_named` reports them handled.

## The part that was not the code: the fixture table

The translator dispatches on instruction **names**, and a name only exists once the decoder's
`mnemonics.toml` maps its `(family, opcode)`. That table is generated from observed fixtures - "an
unobserved name is a guess" - so wiring the translator was inert until the decoder learned the two
names. They were not in the table: no fixture exercised them.

Adding them meant regenerating, which needs LLVM with the AMDGPU backend. The documented path
(`tools/toolchain/setup.sh`) builds a multipass VM; none existed here. Standing one up is 4 GB for a
job a container does in seconds - so the regen ran through `silkeh/clang` plus the generator's own
`--transcript` replay mode.

**The version turned out to be load-bearing.** LLVM 19 disassembles the *existing* `arith.ll` against
a different kernarg base register (`s[6:7]` vs the committed `s[4:5]`), which would churn the first
word of every compute fixture. LLVM 18 - what Ubuntu 24.04's apt ships, what the VM would have used -
reproduces the committed bytes exactly. Pinned and reasoned in **D681**.

New source `tools/shader-fixtures/minmax.ll` lowers `llvm.maxnum`/`llvm.minnum`, which the compiler
turns straight into `v_max_f32_e32`/`v_min_f32_e32` (a compiler-reached fixture, stronger than a
hand-written one). Regenerated to a scratch tree first and diffed against the committed fixtures: every
existing fixture byte-identical, `mnemonics.toml` gaining exactly `(VOP2,15) -> v_min_f32_e32` and
`(VOP2,16) -> v_max_f32_e32`, and the new `minmax.{gcn,txt}`. The empty diff on everything else is what
proves the transcript is a faithful recording of the reference. `minmax` is registered in the
differential suite's fixture list.

## Verified

- `orbistoun-shader`: the differential decoder test now checks `minmax` against the reference and
  passes - the two new instructions' boundaries and names are reference-confirmed.
- `orbistoun-translate` compute oracle: `float_min_and_max_produce_the_right_bits` dispatches
  `max(1.0, 2.0)` and `min(1.0, 2.0)` and reads back `2.0` and `1.0` as bits. This exercises the whole
  ext-inst path end to end - a dropped set import or a wrong instruction number fails here, not forty
  thousand frames later.
- Full `orbistoun-translate`, `orbistoun-gpu-vulkan`, `orbistoun-spirv` suites green; identity guard
  clean.

## What this opens

The same `f32_ext_binary` path now reaches the rest of `GLSL.std.450` with only a handler and a fixture
line each: `FAbs`, `Sqrt`, `InverseSqrt`, and the transcendentals. Each needs its mnemonic observed the
same way - add the intrinsic to a fixture source, regenerate on LLVM 18, diff.
