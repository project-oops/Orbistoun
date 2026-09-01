// Parameter interpolation and export, for solving VINTRP's and EXP's operand layouts.
//
// # Why these were the last two families with no probes
//
// Neither can be *translated* without the graphics pipeline: an export has to know where
// it is exporting to, and an interpolation has to know how attribute memory reaches the
// shader. That is a genuine dependency and it has not moved.
//
// Decoding them is a different question, and nobody had asked it. Both assemble cleanly
// for this target, so their operand layouts are solvable now - and until they are, both
// decode with *no operands at all*, which is the shape that passes a differential test
// vacuously. Four opcodes were sitting in the silent-empty inventory for want of a probe
// file.
//
// # The symbolic operands
//
// These two families are the only ones whose operands are not registers or numbers.
// `attr3.y` names an attribute and a channel within it; `mrt0` and `pos0` name an export
// target; `p10` names one of the interpolation parameters. Their codes are derived by the
// generator rather than written down here - it holds everything else constant, varies the
// name, and reads the bits that moved.
//
// # Registers
//
// Uncorrelated, and reaching past 128, for the reasons the typed-buffer probes give at
// length: a field solved only from low registers solves narrow, and a field whose
// neighbours never vary has readings that cannot be separated.
//
// The attribute numbers stop at 31, and that is a measured bound rather than a habit.
// Probing higher was tried on the assumption the field was six bits wide; the assembler
// refused `attr47`, `attr52` and `attr61` as "out of bounds interpolation attribute
// number". So five bits is the field, thirty-two is the count, and there is nothing above
// it to reach - which is a better answer than the one being looked for.

// Interpolation, first parameter. Destination, source and attribute all vary.
v_interp_p1_f32_e32 v0, v1, attr0.x
v_interp_p1_f32_e32 v100, v37, attr12.y
v_interp_p1_f32_e32 v200, v55, attr31.z
v_interp_p1_f32_e32 v13, v240, attr7.w
v_interp_p1_f32_e32 v250, v3, attr30.x
v_interp_p1_f32_e32 v44, v199, attr5.z

// Interpolation, second parameter.
v_interp_p2_f32_e32 v2, v3, attr1.y
v_interp_p2_f32_e32 v130, v77, attr18.w
v_interp_p2_f32_e32 v9, v201, attr3.x
v_interp_p2_f32_e32 v190, v42, attr31.z
v_interp_p2_f32_e32 v66, v11, attr9.y

// Interpolation, moving a parameter rather than interpolating one. The second operand is
// one of the parameters by name rather than a register.
v_interp_mov_f32_e32 v4, p10, attr2.z
v_interp_mov_f32_e32 v150, p20, attr14.x
v_interp_mov_f32_e32 v88, p0, attr29.w
v_interp_mov_f32_e32 v220, p10, attr6.y
v_interp_mov_f32_e32 v17, p20, attr23.z

// Export. Four sources, and a target that is not a register.
exp mrt0 v0, v1, v2, v3
exp mrt1 v4, v5, v6, v7 done
exp mrt3 v100, v37, v55, v200
exp mrt7 v130, v9, v240, v66
exp pos0 v8, v9, v10, v11 done
exp pos1 v201, v42, v11, v77
exp param0 v190, v13, v250, v44
exp param4 v3, v199, v88, v150
exp mrtz v17, v220, v66, v130 done
exp null v250, v3, v199, v44
