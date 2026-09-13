# 526. Unary and transcendental vector float ALU wired through the extended set

**2026-09-12** - advancing the GPU translate pipeline offline while hardware remains down

Worklog 525 recorded the implementation plan for unary vector float arithmetic and transcendentals.
This executes that plan: wiring `v_sqrt_f32_e32`, `v_rsq_f32_e32`, `v_sin_f32_e32`, `v_cos_f32_e32`,
`v_exp_f32_e32`, and `v_log_f32_e32` through the `GLSL.std.450` extended instruction set via
`OpExtInst`, regenerating differential fixtures via the pinned LLVM 18 reference container (D681),
and verifying end-to-end execution against the Vulkan compute oracle.

## What landed (orbistoun-translate)

- `Model::f32_ext_unary(instruction, operand)` provides single-operand extended instruction emission:
  reinterprets register bits as `f32` via `OpBitcast`, emits `OpExtInst` into `glsl_set()`, and bitcasts
  back to `u32` register bits.
- `float_unary` dispatches the VOP1 float instructions to their respective `GLSL.std.450` instruction IDs:
  - `v_sqrt_f32_e32` -> `Sqrt` (31)
  - `v_rsq_f32_e32` -> `InverseSqrt` (32)
  - `v_sin_f32_e32` -> `Sin` (13)
  - `v_cos_f32_e32` -> `Cos` (14)
  - `v_exp_f32_e32` -> `Exp2` (29) (verified against Khronos `GLSL.std.450.h`)
  - `v_log_f32_e32` -> `Log2` (30) (verified against Khronos `GLSL.std.450.h`)
- `SUPPORTED` updated with all six new instruction names in alphabetical order.
- Compute oracle tests added to `execute.rs`:
  - `float_unary_sqrt_and_rsq_produce_the_right_bits`: asserts `sqrt(4.0) == 2.0` and `rsq(4.0) == 0.5`.
  - `float_unary_transcendentals_produce_the_right_bits`: asserts `exp2(3.0) == 8.0` and `log2(8.0) == 3.0`.
  - `float_unary_trig_produces_the_right_bits`: asserts `sin(0.0) == 0.0` and `cos(0.0) == 1.0`.

## Reference Fixtures & Toolchain (orbistoun-shader)

- `tools/shader-fixtures/unary.ll`: Added LLVM IR kernel lowering `@llvm.amdgcn.sqrt.f32`,
  `@llvm.amdgcn.rsq.f32`, `@llvm.amdgcn.sin.f32`, `@llvm.amdgcn.cos.f32`, `@llvm.amdgcn.exp2.f32`,
  and `@llvm.amdgcn.log.f32` straight onto their respective VOP1 short forms.
- Replayed through `silkeh/clang:18` container per D681:
  - Zero churn on existing fixtures (`arith`, `minmax`, etc. byte-identical).
  - `crates/orbistoun-shader/data/mnemonics.toml` gained exactly the 6 observed VOP1 opcodes:
    - `(VOP1, 37) -> v_exp_f32_e32`
    - `(VOP1, 39) -> v_log_f32_e32`
    - `(VOP1, 46) -> v_rsq_f32_e32`
    - `(VOP1, 51) -> v_sqrt_f32_e32`
    - `(VOP1, 53) -> v_sin_f32_e32`
    - `(VOP1, 54) -> v_cos_f32_e32`
  - Committed `unary.gcn` (256 bytes) and `unary.txt` (61 instructions).
  - Registered `unary` in `differential.rs`; all 7 differential tests passing.

## Verification

- `cargo test -p orbistoun-shader`: 60 unit tests, 7 differential tests, 10 hostile tests, 2 measured shader tests all PASS.
- `cargo test -p orbistoun-translate`: 13 unit tests, 2 agreement tests, 112 execute compute oracle tests all PASS.
- `cargo test -p orbistoun-gpu-vulkan`: all 16 tests PASS.
- `cargo test -p orbistoun-spirv`: all 17 tests PASS.
