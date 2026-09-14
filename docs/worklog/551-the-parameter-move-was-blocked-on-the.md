# 551. The parameter move was blocked on the wrong obstacle

**2026-09-14** - orbistoun-translate, orbistoun-spirv and orbistoun-gpu-vulkan, after worklog
550

`v_interp_mov_f32` has been in `BLOCKED` since the interpolation work, with this reason: its
operand selects between P0, P10 and P20, *and this repository has no citable encoding for
which value names which*. That was wrong. The codes had been measured all along, by the same
solver that measured the export targets - `p10` is 0, `p20` is 1, `p0` is 2 - and the
committed fixture holds two of them in bytes this project disassembled itself. What actually
blocked the instruction was something else entirely, and naming the wrong obstacle kept it
parked for two phases.

It translates now, for the parameter that has a host counterpart, and refuses the two that do
not. `BLOCKED` is down to the two subsystems.

## 1. The real obstacle: interpolation is a property of the variable

The move reads a parameter straight out of the cache the hardware filled, without
interpolating. SPIR-V has no parameter cache. An input variable **is** the attribute, and
whether reading it interpolates is decided by a decoration on the variable - which is written
in the module header, before any instruction is translated (D555).

So the whole translation of this instruction is a decoration somewhere else, and the code at
the instruction is the same component read the interpolating pair already does. That is why
it could not be done at the time: the declaration pass recorded *which* attributes a shader
reads and not *how*, and the instruction had no way to reach back and change it.

The pass now decides both. An attribute a shader reads **both** ways is refused rather than
resolved by picking, because one variable carries one decoration and reading a flat parameter
through an interpolated input is a different number everywhere except at one vertex.

## 2. What P0 is, without a new source

The three parameters are not a mystery given the interpolation this translator already
implements: `v_interp_p1_f32` computes `P10 * I + P0` and `v_interp_p2_f32` adds `P20 * J`, so
`P0` is the value where both barycentrics are zero - the first vertex of the primitive - and
the other two are deltas from it. A shader that wants an attribute unchanged across a
primitive asks for `P0`, which is flat shading with a first-vertex provoking convention, and
that is the convention the host uses by default.

`P10` and `P20` are refused with their own reason: the host interpolates with its own
barycentrics and offers the result, never the gradient it used. A shader moving a delta into a
register is doing arithmetic with the primitive's shape, and answering with the attribute's
value would be a plausible number that is wrong everywhere.

## 3. Measured on a GPU, as a differential

`tests/translated_interpolation.rs` gains two tests. The important one gives the three corners
**different** colours, so an interpolated read is a gradient and a flat read is one colour,
and draws both from translated modules over the same geometry:

- every pixel of the flat draw is identical, which is what flat means and does not depend on
  any convention;
- that colour is the first corner's, which is the provoking vertex under the host's default;
- and the interpolating translation of the same attribute over the same corners is **not**
  uniform, asserted in the same test, because without that the first assertion could be
  measuring the geometry rather than the decoration.

The second test is the refusal: moving `P10` does not translate. Both ran against an NVIDIA
GeForce RTX 5070 Ti, along with the two interpolation tests that were already there.

The instruction words come from the committed fixture rather than from a document: the two
bits of the family's opcode field and the parameter codes are read off
`unreached.gcn`, which holds `v_interp_mov_f32_e32 v4, p0, attr0.x` as `0xc8120002` and
`v9, p10, attr31.w` as `0xc8267f00`.

## 4. The lesson, which is one this project already has a rule for

"A message naming a cause must come from the branch that determined it." A blocked entry is a
message naming a cause, and this one named an encoding gap where the obstacle was a structural
one two files away. The cost was not the wrong sentence; it was that the entry looked like it
needed an external source, which is the one kind of blocker nobody here can clear, so nobody
looked at it again.

The doc comment on `BLOCKED` now records that, beside the two entries that remain.

## 5. Also filed: the probe D688 depends on

`REQ-20260914T1720Z-9c4a` on the obSCEne bus asks what the fields of the `MSG_GS_ALLOC_REQ`
payload are. D688 makes an NGG primitive shader a mesh shader, whose first act is declaring
how many vertices and primitives the workgroup emits - that declaration is this register's
payload, and one sample (`0x1003`, one primitive of three vertices) cannot separate the
assumed split from several others. The request runs four variants of oops-gl's own vertex
program and reports which draw.

## Files

- `crates/orbistoun-spirv/src/lib.rs` - the `Flat` decoration.
- `crates/orbistoun-translate/src/wavefront.rs` - `Interpolation`, the mode in the discovery
  pass, the conflict refusal, and the decoration at declaration.
- `crates/orbistoun-translate/src/model.rs` - `parameter_move`, the measured parameter code,
  the name supported, the entry gone from `BLOCKED`, and the doc that explains why it was
  there.
- `crates/orbistoun-gpu-vulkan/tests/translated_interpolation.rs` - the flat differential and
  the delta refusal.

## Next

1. `REQ-20260914T1402Z-b6d8`, the flat base - still the only blocker on the GL cube's own path
   that is neither a subsystem nor a decision.
2. `REQ-20260914T1720Z-9c4a`, filed today, which settles D688's one assumption.
3. The mesh stage and an image subsystem, in that order - the first has a decision behind it.
