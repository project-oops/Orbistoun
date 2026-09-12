# 523. SPIR-V extended-instruction support: the unlock for min/max and the transcendentals

**2026-09-12** - hardware down; advancing the GPU pipeline offline (framebuffer/compute oracle, no PS5)

The RDNA2->SPIR-V translator handles a good core-opcode set but stops at anything that lives in the
`GLSL.std.450` extended instruction set - `model.rs` says so directly: "absolute value is only in an
extended instruction set this crate does not support." That set is where `FMax`, `FMin`, `FAbs`,
`Sqrt`, `InverseSqrt`, `Exp`, `Log`, `Floor` and the rest are, so its absence caps how many real shader
instructions can translate. This adds the capability those all need.

## What landed (orbistoun-spirv)

`OpExtInstImport` (opcode 11) and `OpExtInst` (opcode 12), with the builder plumbing to use them
correctly:

- `Builder::ext_inst_import(name) -> Id` imports a set (e.g. `"GLSL.std.450"`) and returns its id. It
  is placed in the header at the slot the logical layout requires - after `OpExtension`, before
  `OpMemoryModel` (SPIR-V 2.4) - by extending `header_slot`/`HEADER_SLOTS`, so a caller cannot put it in
  the wrong place regardless of call order.
- `Builder::ext_inst(result_type, set, instruction, operands) -> Id` emits `OpExtInst` in the function
  section, in the format's word order (result type, result, set, the instruction number as a literal,
  then operand ids).
- `Shape::of` learns both opcodes so `check()` recognises them - `OpExtInstImport` defines its result at
  index 0 (the rest is a literal string, no ids); `OpExtInst` defines at index 1, uses the type (0) and
  set (2), and treats everything from index 4 as ids, skipping the literal instruction number at 3.

Two tests pin it: the import lands before the memory model whatever the call order, and an `OpExtInst`
carries result-type/result/set/instruction/operands in the exact order. `cargo test -p orbistoun-spirv`
green (17), and the header-slot renumbering is regression-free: an empty extended-import slot contributes
nothing, so every existing module's word stream is unchanged - `orbistoun-translate` and `orbistoun-gpu`
suites both green.

## Why it stops here, and the next step

This is the foundation; the caller is the immediate next step and is fully scoped:

- Add a `glsl_set() -> Id` method to the `Model` trait, import `GLSL.std.450` once during module setup
  (`wavefront.rs` `emit_header`/init) and return its id from the impl.
- Add a provided `f32_ext_binary(instruction, lhs, rhs)` mirroring `f32_binary` (bitcast u32->f32, call
  `ext_inst` instead of a core op, bitcast back).
- Dispatch `"v_max_f32_e32" | "v_min_f32_e32"` to a handler calling it with GLSLstd450 `FMax` (40) /
  `FMin` (37) - the two most common ALU ops still unhandled, in nearly every real shader (clamp/saturate).
- **A compute test** that dispatches a shader using `v_max_f32`/`v_min_f32` against known inputs and reads
  the result back - the oracle that proves the translation *computes* max/min, not merely that it
  validates. Getting that verification right is why the wiring is a deliberate next step rather than
  rushed: a wrong ext-inst number renders nonsense a validator would accept.

Once wired, the same path opens `FAbs`, `Sqrt`, `InverseSqrt` (the measured `v_rcp` cousin), and the
other transcendentals - a broad, framebuffer/compute-verifiable coverage gain with no hardware.

## State

- `orbistoun-spirv` ext-inst capability added + tested; downstream green; guard clean. Uncommitted.
