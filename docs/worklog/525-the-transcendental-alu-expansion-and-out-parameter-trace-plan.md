# 525. The transcendental float expansion plan and the allocator out-parameter trace

**2026-09-12** - filed while hardware is down; queued for the GPU translate worklist

With `v_min_f32_e32` and `v_max_f32_e32` landed and verified against the compute oracle
(worklog 524), and the LLVM 18 container fixture toolchain pinned in D681, the path to
the rest of the unary and transcendental vector float operations is clear. This entry
records the planned implementation and the queued retail trace work so the session can
pivot to offline `oops-sdk` pipeline and `obSCEne` probe pre-authoring.

## 1. Unary Extended Float ALU (orbistoun-translate)

The `Model` trait currently provides `f32_ext_binary` for two-operand extended instructions
into the lazily-imported `GLSL.std.450` set. The counterpart for single-operand operations
is `f32_ext_unary(instruction, operand)`:

- Reinterprets the 32-bit register operand as an `f32` via `OpBitcast`.
- Emits `OpExtInst` into `glsl_set()` for the requested `GLSL.std.450` instruction number.
- Reinterprets the resulting float back into unsigned 32-bit register bits via `OpBitcast`.

### Target Mappings

| RDNA2 GFX10.3 Instruction | LLVM Intrinsic | GLSL.std.450 Instruction | ID |
| :--- | :--- | :--- | :--- |
| `v_sqrt_f32` | `@llvm.sqrt.f32` | `Sqrt` | 31 |
| `v_rsq_f32` | `@llvm.amdgcn.rsq.f32` | `InverseSqrt` | 32 |
| `v_abs_f32` | `@llvm.fabs.f32` | `FAbs` | 4 |
| `v_sin_f32` | `@llvm.sin.f32` | `Sin` | 13 |
| `v_cos_f32` | `@llvm.cos.f32` | `Cos` | 14 |
| `v_exp_f32` | `@llvm.exp2.f32` | `Exp2` | 28 |
| `v_log_f32` | `@llvm.log2.f32` | `Log2` | 27 |

### Fixture Generation Protocol

As pinned in D681:
1. Write `tools/shader-fixtures/unary.ll` exercising the LLVM intrinsics.
2. Run compilation and disassembly inside the `silkeh/clang:18` container to produce
   `<recdir>/fixtures-unary.out` without churning kernarg base registers (`s[4:5]`).
3. Run `cargo run -p orbistoun-gen -- --transcript <recdir> fixtures` to produce
   `unary.gcn`, `unary.txt`, and append the observed names to `mnemonics.toml`.
4. Wire dispatch in `model.rs` and verify end-to-end via compute oracle tests in
   `crates/orbistoun-translate/tests/execute.rs`.

---

## 2. The Terminator Allocation Out-Parameter Trace

Worklog 522 identified that the `int 0x41` trap is a Unity `TempOverflow` assert trying
to allocate `105553124636954` bytes (`0x6000_007F_BEDA` - a stack address used as a size).

The queued investigation is an IL2CPP trace from the allocation site at `Line:543` back
to the memory/content query whose out-parameter was not filled:
- Audit all early system service calls made by the main thread prior to the trap.
- Check any out-parameters passed as pointers in the `0x6000_007fbe**` range.
- Fill the measured size/quota so Unity allocates from its configured heap rather than
  interpreting leftover stack frames as 96 TiB requests.

---

## State

- Worklog filed to preserve context across working sessions.
- Submodule remains green and clean; offline development continuing on `obSCEne` Probe 169
  and `oops-sdk` OpenGL depth/texture pipeline integration.
