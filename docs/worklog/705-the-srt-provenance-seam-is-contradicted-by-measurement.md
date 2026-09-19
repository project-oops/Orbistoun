# 705. The SRT provenance seam its premise asks for is contradicted by measurement

**2026-09-19** — inbox `-5f89`: an architectural request to build a Shader Resource Table / descriptor
provenance seam between the AGC front-half and the shader recompiler, on the stated premise that "in
RDNA2 ISA, memory operations carry no data format tags in the bytecode; the formats live exclusively
in the descriptors," and that "without descriptor provenance, vertex and texture shaders cannot be
recompiled correctly." Both halves of that premise are false in the tree, by measurement and by a
rendered frame. Closing on the measurement, the honest disposition (as `-735b` and `-4b1a` were).

## The format is in the bytecode, not the descriptor

A typed buffer access carries its format as a **seven-bit operand at bits 25:19 of its first word**
(`crates/orbistoun-shader/src/formats.rs`, `SHIFT = 19`), not in a V#/T# descriptor. Its 77 codes were
measured by asking the reference assembler for each and reading the field back
(`crates/orbistoun-shader/data/buffer-formats.toml`). The translator reads the format from there and
emits the matching unpack and conversion - integer, normalised, scaled and half-float, in one word or
two (worklogs 586-588) - and refuses a code it has not measured by name (`-4c2e`). This is exactly the
acceptance's outcome ("resolves its format … to emit typed SPIR-V loads"), reached by a **better**
route than the one it proposes: a bytecode field the instruction carries, needing no provenance table
at all. `tests/execute.rs` exercises `tbuffer_load_format_xyzw` across `BUF_FMT_32_FLOAT`,
`8_8_8_8_UNORM`, `10_11_11_FLOAT`, `11_11_10_FLOAT`, `10_10_10_2_UINT`, `2_10_10_10_UINT` and more, each
a measured code.

(A smaller correction the acceptance names an instruction that is not a mnemonic: it is
`tbuffer_load_format_xyzw`, not `buffer_load_format_xyzw`.)

## The recompiler already produces valid SPIR-V without the seam

The premise's strongest claim - "without descriptor provenance … shaders cannot be recompiled
correctly" - is refuted by a rendered frame. `-f50b` (worklog 701) walked the console's triangle,
translated **its own GCN vertex and pixel shaders**, and drove them through the backend to reproduce
the console's 512 drawn pixels **exactly**. The recompiler emits valid, compilable SPIR-V today, with
no SRT provenance side-table: its memory model is a flat storage buffer over a bound window, and the
descriptor's own data - base, record count, stride, access mode - is decoded from its four scalar
registers where it is needed (D204), not tracked through an SGPR provenance graph. So neither the
direct (`sgpr_base`) nor the indirect (`srt_offset`) provenance the request specifies is a bridge
anything is missing.

## Disposition

**Resolved `not-needed`.** The seam's job - a typed SPIR-V load from a resolved format - is done and
tested by a mechanism the request's premise did not know about (the bytecode's format field), and the
recompiler it was meant to unblock is already producing correct frames. Building an SGPR→descriptor
provenance table for *format* would add machinery to resolve what is already resolved. If a future
need appears for the descriptor-only fields to flow as *provenance* rather than register decode, that
is a different request with its own measurement behind it, not this one carried out as written.

## Gate state

No code changed - this is a verification and a close, against cited, tested code that exists in the
tree (`formats.rs`, `buffer-formats.toml`, `tests/execute.rs`, and f50b's console-triangle render). No
commit.
