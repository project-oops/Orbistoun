# D880 - A dispatch that never meets its wave runs one invocation per lane

**Status:** decided
**Date:** 2026-10-14

A guest compute dispatch whose program never treats its wave as a whole runs one invocation per
lane: the lane model automatic fidelity picks for it. Such a program has no instruction that reads
or writes the execution mask, reads a lane mask whole, reads another lane's value (a lane read, a
permute, a swizzle, a DPP modifier), uses the local data share, waits at `s_barrier`, or stores or
makes an atomic through the scalar unit. `lane_exact::lane_needs` names which of these a program
has, and any one keeps the whole wave in one invocation, as before, with the slowest-fidelity
warning saying which. Asked to run per lane, such a program is refused by name. Amended by D973:
where the host's subgroup carries the wave, its width or half of it, every one of these but a scalar
store or atomic runs per lane too. Masks are the subgroup's ballots, another lane's value is its
shuffle, and the wave's order at the local data share is the subgroup's barrier. A wave of
sixty-four on a subgroup of thirty-two runs two lanes an invocation.

**The workgroup.** A group of `n` waves is a host workgroup of `n` times the wave's width.
Invocation `i` is lane `i mod width` of wave `i / width`, and its thread id is its wave's first
thread plus its lane, x fastest, as D858 places it. The host's subgroup is not used, so its width
need not match the wave's: a wave64 program runs on a 32-wide device.

**The scalar state.** Each invocation computes every scalar register as its wave would, so control
flow stays uniform within the wave. Nothing is stored through the scalar unit, so nothing is made
once per lane.

**The masks.** Each invocation holds only its own lane's bits. Its execution bit is clear for a lane
past its group's last thread or outside a partial last group (D855, D858) and is never written
after. A comparison or a carry writes its lane's bit into the mask register, and a selection or a
carry-in reads that bit back; nothing in such a program reads a mask whole, so no other lane's bit
is wanted.

**Memory.** A lane that does not run, and a store past its buffer's bound words, store nothing, where
one invocation per wave wrote back the value it had read: another invocation may be storing that
word at the same time. The same holds for every guest dispatch, whose waves and groups have always
run as invocations side by side: a 100-thread group's second wave has 28 lanes past the group's
end whose thread ids wrap onto the first wave's, and their written-back words raced the first
wave's stores.

The lanes of a wave are not ordered against one another's memory accesses, as they are within one
invocation. A program that reads what another lane of its wave stored, with no barrier between,
depends on that order. HLSL and GLSL leave such a read undefined, and their compilers place a
barrier where they need one; this is accepted rather than detected.

**Why:** PPSA28061 dispatches one compute program, and running it was a third of its run (`perf`,
30 s as played): `device: run` 32.7%, 11.1 ms a dispatch, 27.1 fps. The program uses none of the
above. Run per lane, a dispatch's device time is 1.65-1.73 ms against 3.8-6.0 ms for the whole wave
in one invocation with the same stores, and the run 36-43 fps against 18-23 at the revision before
(two 30 s runs of each). The frames are the same: every flip both kept, 56 under the logical clock,
is byte for byte the whole wave's. Of the gain over that revision, 15-24 ms a dispatch to 4-6 ms
is the stores alone: each lane read the word it would write, to write it back unchanged where it
did not run. Across the library's titles, 9 of the 14 compute programs their kept translations
hold run per lane exactly (`orbistoun-cli lanes`); four of the other five read a mask whole and
nothing else, and one also uses the local data share.

**Rejected:**
- Every lane's bits by subgroup ballot, or through workgroup memory, as D760 does for primitive
  shaders: no instruction of these programs reads them, and a group of several waves would place
  each exchange's barriers in control flow its waves do not share.
- The predicated lane model: it has none of a dispatch's entry state, buffers, images or window,
  and building them beside the wavefront model's would be a second account of guest memory.
- Subgroup fidelity: wave64 needs a 64-wide subgroup, which the NVIDIA device does not offer.
