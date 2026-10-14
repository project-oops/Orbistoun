# D884 - A dispatch that reads a lane mask whole builds it from its lanes

**Status:** decided
**Date:** 2026-10-14

A guest compute dispatch whose program reads or writes a lane mask whole, and needs nothing else
of its wave, runs one invocation per lane (D880) with every lane's bits in each invocation: the
mask a comparison, a carry or the execution mask's seed builds is the wave's whole mask, made from
each lane's own bit as D760 makes a primitive shader's. Every scalar operation on a mask - a branch
on `execz` or `vccz`, a `saveexec`, a lane count - then reads the value the wave holds, the same in
every lane's invocation, and the program's scalar control flow stays uniform within its wave.

**Two exchanges, by what the host offers:**
- *Ballot*, where the host's subgroup is the wave's width: the mask is the subgroup's ballot of
  each lane's condition. An invocation's lane is its place in its subgroup and its wave is its
  subgroup, so a group of several waves is as many subgroups and needs nothing more. Wave32 on
  NVIDIA, whose subgroup is 32 wide; wave64 on a 64-wide subgroup.
- *Workgroup memory*, where the subgroup is narrower and the group is one wave: D760's exchange,
  each lane's bit ored into a two-word mask between workgroup barriers. A group of one wave is one
  workgroup whose invocations share one scalar control flow, so every barrier is met by all of
  them.

A group of several waves on a subgroup narrower than its wave keeps the whole wave in one
invocation, with the reason said: its waves' scalar control flows differ, and an exchange's
barriers inside one wave's branch would not be met by the others'. So does a program that also
needs any other thing D880 names - another lane's value, the local data share, a barrier, a scalar
store.

**The execution mask's seed.** A group of one wave with no partial groups seeds the mask as a
constant, as before. Otherwise each lane's bit is whether its thread runs (D855, D858), and the
whole mask is made from those bits by the same exchange as any other.

**The gate.** `lane_exact::lane_masks` says how a dispatch run per lane holds its masks - its own
bits, by ballot, or through workgroup memory - from the program's needs, its wave width, the host's
subgroup and its group's waves, or why it cannot run per lane. The translation's choice
(`dispatch_per_lane`) and the emitter both read it. The host's subgroup is the one a primitive
shader runs per lane on (`host_subgroup`): a device whose mesh stage takes a subgroup ballot takes
it in compute too, the stage every Vulkan 1.1 device's subgroup supports. A module translated for
one subgroup is another module from one translated for another, and its cache key says which.

**Why:** On 2026-10-14, 9 of the library's 22 guest compute programs (`orbistoun-cli lanes`) were
kept whole for a whole lane mask, 8 of them for nothing else: PPSA02664, PPSA03416, PPSA04263,
PPSA25872 and STKT00001. All 8 are wave64, and 7 are groups of one wave, which run per lane on the
NVIDIA device through workgroup memory; PPSA04263's is a group of several and stays whole. Run
whole, one invocation does a wave's lanes in turn; per lane, D880 measured a dispatch's device time
at a third.

**Measured** (`orbistoun-cli ab --against` the revision before, three 30 s rounds each): `device:
run` falls as a share of the run on all four titles - PPSA25872 11.8% to 10.6%, PPSA03416 4.8% to
4.4%, PPSA02664 2.4% to 1.9%, STKT00001 5.5% to 4.8% - and the frame rates move within the spread
of one build's own runs. These programs are guarded copies and fills of a few hundred elements,
not where these titles' time goes; the gain is the model, not a frame rate.

Under the logical clock, frames kept every 30th flip for 60 s by two runs of the revision before
and one of this: STKT00001's 246 are byte-identical; PPSA02664 has no flip both earlier runs agree
on and this one does not; PPSA03416's two earlier runs disagree with each other on 118 of 148. On
PPSA25872 four flips differ from both earlier runs: the splash fade, the same two frames in both
builds, changing at flip 241 in one and 255 in the other - the clock's phase, not a pixel. The
titles' own programs, PPSA03416's and PPSA02664's fill and PPSA25872's offset copy, store the same
elements per lane as whole in the device tests.

**The warning.** A dispatch run per lane is not warned as the slowest fidelity; one that read a
lane mask was, by the generic warning, though it ran per lane.

**Rejected:**
- The exchange in a group of several waves, a mask slot per wave: a barrier is the workgroup's,
  and a wave that branches past an exchange leaves the others waiting at it.
- A host subgroup asked to be 64 wide (`VK_EXT_subgroup_size_control`): NVIDIA offers 32 only.
- Holding only a lane's own bits and refusing the scalar mask operations one at a time: a branch on
  `execz` is the wave's question, which no single lane's bit answers.
