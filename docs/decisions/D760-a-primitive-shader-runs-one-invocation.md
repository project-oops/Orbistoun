# D760 - A primitive shader runs one invocation per lane

**Status:** decided
**Date:** 2026-10-08

A primitive shader runs as a mesh workgroup of its wave's width, each invocation one lane, wherever
the host records its subgroup width. This is the subgroup fidelity D100 and D716 hold for primitive
shaders.

**The register file.** Each invocation holds only its own lane's registers. The per-lane semantics
run once, for the lane the invocation is, and every place a lane number was a constant takes it at
run time: mask bits, `v_mbcnt`'s lanes below, a permute's row, the geometry seeding, and the slot a
vertex or primitive is written to.

**The scalar state.** Each invocation computes the scalar registers and the execution mask
identically, so control flow stays uniform across the workgroup.

**Masks and lane reads.** A mask a vector instruction writes is every lane's bit at once. Where the
host subgroup holds the wave, it is built by subgroup ballot and a lane read is a subgroup shuffle.
Where the wave is wider, as a wave64 shader on a 32-wide device, masks and lane reads go through
workgroup memory between barriers. There the lane is the invocation's place in the workgroup.

**The local data share.** It is workgroup memory, with a barrier after every access standing in for
the wave's program order. A byte write or an or merges by atomics, so lanes sharing a word keep each
other's bits. An inactive lane's store is skipped rather than written back.

**What keeps the whole wave in one invocation.** A rectangle list's copies, and a lane permute other
than `v_permlane16`.

**Why:** a primitive shader simulated in one invocation is the whole wave's work done serially, in
modules of megabytes. CRFT00001's wave64 primitive shaders were 13-19 MB disassembled. Each new
pipeline took 13 to 73 s to build, and the device was busy about 880 ms of every second for 40,000
draws a frame, at 0.5 to 1 frames a second. Run per lane, the same frames build in under a second,
and the device is busy about 9 ms a second. The pixels are the same, which a device test of both
widths checks against the whole-wave model.

**Rejected:**
- Ballot and shuffle only, refusing a wave wider than the subgroup: radeonsi's primitive shaders are
  wave64, and NVIDIA's subgroup is 32 wide.
- Requiring a 64-wide subgroup through subgroup size control: the device offers only 32.
- Keeping the whole-wave model where the local data share is used: radeonsi's NGG shaders all use it.
