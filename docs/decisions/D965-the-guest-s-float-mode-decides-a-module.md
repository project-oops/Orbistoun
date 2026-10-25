# D965 - The guest's float mode decides a module's float controls

**Status:** decided
**Date:** 2026-10-25
**known_by:** published

A program's `FLOAT_MODE` (PGM_RSRC1 bits 19:12, for compute and for every graphics stage) is
translated into SPIR-V float controls: 32-bit "keep" becomes `DenormPreserve 32` and "flush"
`DenormFlushToZero 32`. The partial modes - keep on input only, or on output only - cannot be
expressed; they are translated by their outputs, and the translation says so. The mode is part of
the module's cache key. Every module also declares `SignedZeroInfNanPreserve 32`. Each mode is
declared only where the device offers it; the device's float-controls properties are read once, a
module asking for one it does not offer gets the nearest it does, or none, and the log says so once
a module. Compute programs read `COMPUTE_PGM_RSRC1`'s own field rather than assuming flush to zero.
A device that offers neither mode leaves subnormals to the driver, which may decide per
instruction; tests comparing two translations there allow a subnormal-against-zero difference and
nothing else. Separately, `v_rcp_f32` is computed over every float, scaling a divisor outside
2^±126 by 2^±64, since `OpFDiv` promises nothing there.

The field's layout is Mesa's (`src/amd/registers/gfx10.json` for each stage's PGM_RSRC1, and
`src/amd/compiler/aco_ir.h` for the 32-bit and 16/64-bit subnormal bits).

**Why:** lane-loop fusion (D956) changed which code shape a subtraction of two subnormals lands
in, and with no mode declared the driver flushed it in one model and kept it in the other: the
result depended on the shape of the code, not on the guest. The guest says what it wants; the
module should carry it. Without `SignedZeroInfNanPreserve`, a driver may return 0 for infinity
times a number, and declaring float controls a device does not offer produces modules it rejects.

**Rejected:**
- Assuming flush to zero for every compute program: a program that keeps subnormals computes
  differently.
- Flushing by hand after each instruction: a cost on every float result, where the device's own
  mode costs nothing.
