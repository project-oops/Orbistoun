# 545. The GL cube oracle through the submission pipeline: m0 lands, five names and one base are missing

**2026-09-14** - orbistoun-gpu and orbistoun-translate, after worklog 539 (the oracle record)
and 541 (the prior-art audit)

The two records oops-sdk took from the console on firmware 12.40 - record A, the untextured
cube, hash `0x9dbfe189`; record B, textured, hash `0xc51cec32` - are now captures under
`crates/orbistoun-gpu/tests/captures/` and go through the whole submission path in
`tests/oracle_gl_cube.rs`: walk, register writes, shader resolution against the payload image
as it sat at `0x2008f0000`, translation. Getting them there cost four corrections to the
packet vocabulary, each decided by the capture. The pipeline now resolves both shaders in
both records and translates none of them, and the reasons are the measurement this unit
exists to take: one operand the model had no slot for (fixed here), five opcode names the
fixture corpus never produced, one operand family with no layout, and one flat base code the
console accepted and nobody has measured.

## 1. Four vocabulary corrections, each from the capture

1. **`SET_UCONFIG_REG_INDEX` (opcode 0x7a) was not in `data/packets.toml`.** The GL path
   writes `VGT_PRIMITIVE_TYPE` through it, with index 1 in bits 31:28 of the offset word
   (`0x10000242`). Added as an opcode and a register-write command at base `0xC000`, and
   `register_writes` now keeps only the low sixteen bits of the offset for every
   register-write command. Read whole, that word placed the write at `0x1000C242`, a register
   that does not exist.
2. **The fragment shader's address rows were wrong.** The table had `SPI_SHADER_PGM_LO_PS`
   and `_HI_PS` at `0x2C0C`/`0x2C0D`. The capture writes the shader address at
   `0x2C08`/`0x2C09`, and `0x2C0C`/`0x2C0D` are `SPI_SHADER_USER_DATA_PS_0`/`_1` - record B
   carries the T#/S# table pointer there. Corrected, citing the capture. `known_by measured`.
3. **The address in `PGM_LO`/`PGM_HI` is in 256-byte units.** `0x02008f03` is
   `0x2008f0300`. `shader_candidates` now shifts the joined value left by eight;
   `tests/pipeline.rs` and `tests/shader_capture.rs` encode their addresses the same way.
   Before this every resolved address was 256 times too small and pointed at nothing.
4. **A count field of all ones is a header-only packet.** obSCEne's `sceAgcCbNop` writes
   four bytes, `0010ffff` (probe `166-agc/cb-nop`), and the GL cube stream ends in sixteen
   `0xffff1000` words after its fence event. The walker read each as a 64 KiB packet and
   overran the buffer. `the_no_op_is_the_one_that_does_not_close` in
   `tests/measured_packets.rs` had recorded that disagreement rather than smooth it over; it
   is now `the_no_op_closes_as_a_header_only_packet` and asserts the rule, and a unit test in
   `packet.rs` walks a header-only word followed by a real packet.

With those in place both captures walk clean (357 packets, 310 register writes each), the
vocabulary test accepts every expectation (fourteen registers in A, eight in B), both shader
addresses resolve inside the payload image, the two routes never disagree, and no write names
a stage the draw queue cannot run.

## 2. What the translator says, ranked by what unblocks most

| shader | stops at | instruction | needs |
|---|---|---|---|
| vertex program, both records | `0x0`: no recorded name for that opcode | `s_inst_prefetch` (SOPP 0x20) | a fixture that names it |
| untextured pixel shader, A | `0x30`: flat access has a base this translator does not understand | `global_store_dword v[8:9], v10, null offset:4` | a measurement (§6) |
| textured pixel shader, B | `0x84`: no operand layout | `image_sample_lz` (MIMG 0x27) | operand-solver probes for MIMG, of which there are none for any opcode |

The census (`orbistoun-cli shaders` over the six binaries) counts 186 of 200 instructions
translatable by name and six unnamed keys: SOPP 0x20 `s_inst_prefetch`, SOPP 0x0 `s_nop`,
SOPP 0x10 `s_sendmsg`, VOP2 0x16 `v_lshrrev_b32`, VOP2 0x28 `v_add_co_ci_u32` in its
two-operand form (the three-operand form is supported), MIMG 0x27 `image_sample_lz`. Five of
the six are the vertex program's. Names come from `orbistoun-gen fixtures`, which assembles
`tools/shader-fixtures/` with `llvm-mc` in the toolchain VM and has a reference disassembler
name what it built - tooling time, not console time, and none of the six is obscure.

Naming them is not the end of the vertex program. It is an NGG primitive shader: it asks the
geometry engine for space with `s_sendmsg MSG_GS_ALLOC_REQ`, exports a packed primitive with
`exp prim` from one lane, then narrows to three lanes for the vertices. What a host vertex
shader does with a primitive export is a concept the decision log does not have. Flagged
here, not assumed.

## 3. m0 lands

The untextured pixel shader's second instruction is `s_mov_b32 m0, s0` - the primitive mask
the interpolator reads, handed over before the first `v_interp` - and the translator refused
it: `m0` decodes as a named operand (code 124, past the last scalar register), and neither
model had anywhere to put it. Both now hold it as one private word, declared the way the
condition code is; `s_mov_b32` writes it and any thirty-two-bit source reads it back. Neither
model *acts* on it: the interpolator's use collapses into the fragment input (D555), and an
allocation request has no host counterpart. A device test,
`a_move_through_m0_round_trips_and_leaves_the_scalar_file_alone`, runs at both fidelities and
checks the value comes back and that no scalar register was aliased.

## 4. The census reported more than it measured

`shaders 2 of 6 complete`, said the census - the two untextured pixel shaders - while the
pipeline translated zero. "Complete" there means every opcode has a name on the supported
list; both refusals in that shader were at operand level (`m0`, then the flat base), which a
census keyed on opcodes cannot see. The principle-3 case, one level up from the translator.

Not fixed here, and the reason is recorded so it is not re-derived: the census has no stage
information, so asking the translator per shader from there would refuse every fragment
shader at its `exp` for want of an output, which is a different false report. Either the
corpus carries a stage per file or the oracle test is the census for real streams. For these
six shaders the oracle test is the ground truth, and its output is what §2 reports.

## 5. Fidelity is the pipeline's to choose

The test began at lane fidelity, copied from `tests/pipeline.rs`, and the pixel shader stopped
at `0x8`: `s_mov_b32 s4, exec_lo`, saving the mask around a single-lane store, which the
per-lane model refuses by design. A submission is not asked to pick a level, so the test now
uses `Fidelity::Auto`, the pipeline's default. Under it the shader translates nine
instructions and stops at the tenth.

## 6. The flat base: code 125

The store is `0xdc708004 0x007d0a08`: the scalar-base field holds `0x7d`, which the operand
table names `null`. The translator's no-base marker is code 127, `exec_hi`, which is what
`llvm-mc` emits for `off`, and it refuses any other name rather than guess where the access
goes. The console ran this word in every frame of both records without a fault, and the frames
hashed right, but the store's landing - the pixel-shader canary at canary + 4 - is shown on
the cube's HUD and was never logged. So whether `null` in that field means "no base, the
VGPR pair is the address" or "a base of zero added to a thirty-two-bit VGPR" is not on record,
and the two put a store four gigabytes apart. Filed as `REQ-20260914T1402Z-b6d8` on the
obSCEne bus with both encodings and the expected word each way. The translator keeps refusing
until the answer is measured; the alternative fix is oops-sdk emitting `0x7f`, after which
the capture must be retaken and the hash must not move.

## 7. Provenance

Everything here came from oops-sdk's own command stream and hand-assembled shaders (the three
NGG facts in them are cited to open sources under D005), obSCEne probe `166-agc/cb-nop`, and
the published RDNA2 instruction set for field names. No decrypted module was read, nothing was
disassembled, and nothing was taken from the four repositories audited in 541.

## Files

- `crates/orbistoun-gpu/data/packets.toml` - opcode 0x7a and its register-write row; the
  fragment `PGM` rows moved to `0x2C08`/`0x2C09`.
- `crates/orbistoun-gpu/src/registers.rs` - sixteen-bit offset; addresses in 256-byte units;
  unit tests re-pointed.
- `crates/orbistoun-gpu/src/packet.rs` - header-only rule and its unit test.
- `crates/orbistoun-gpu/tests/measured_packets.rs`, `tests/pipeline.rs`,
  `tests/shader_capture.rs` - re-pointed at the measured encodings.
- `crates/orbistoun-gpu/tests/captures/agc-gl-cube-fw1240-{a,b}.{toml,hex,payload.hex}` and
  `tests/oracle_gl_cube.rs` - the records and the test that feeds them through. The words are
  hex text, not `.bin`: these are the first captures ever added, and the captures README's
  `.bin` had never met the provenance guard, which refuses that extension on sight as the
  shape a dump takes. Text is the better record anyway - it reads in a diff - so the README
  and `tests/vocabulary.rs` now say `.hex`, read through `tests/common/mod.rs`.
- `crates/orbistoun-translate/src/{model,predicated,wavefront}.rs` - the `m0` slot;
  `tests/execute.rs` - its device test.
- `crates/orbistoun-gpu-vulkan/tests/dispatch.rs` - encoded its hand-built shader address in
  bytes, so under the 256-byte rule it resolved 256 times too high, into unmapped memory.
  Now in units, as the console writes it. The only consumer the correction broke.

## The tree gate, for the record

`./bin/orbistoun check` passes provenance, the worklog and decision numbering, the generated
tables, the measured-behaviour ledger, fmt, clippy, the workspace tests (533, device tests
included) and doc-tests. Three gates fail on this machine for reasons outside this unit, all
in files this unit did not touch: `prose` lists thirty-one tracked, unmodified files that are
not on its backlog; `status --check` says `README.md` has lost its generated block, and that
file carries a 205-line uncommitted change from another session; `cargo doc` refuses a link to
a private item at `crates/orbistoun-gpu/src/agc.rs:531`, likewise uncommitted and not mine.
The two failures this unit did cause - a `.bin` the provenance guard refused and a
line-continued string the prose gate refused - are fixed above.

## Next

1. Name the five, and give MIMG a layout: extend `tools/shader-fixtures/families/SOPP.s` and
   `VOP2.s`, add an MIMG family file, re-record in the toolchain VM, regenerate the tables.
2. Take the answer to `REQ-20260914T1402Z-b6d8` into `flat_address`, or into oops-sdk's
   encoder, whichever it turns out to be.
3. Decide what a primitive shader is to this translator before touching `s_sendmsg` or
   `exp prim`.
