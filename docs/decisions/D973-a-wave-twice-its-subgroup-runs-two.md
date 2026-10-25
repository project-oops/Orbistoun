# D973 - A wave twice its subgroup runs two lanes an invocation

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A guest compute dispatch whose program treats its wave as a whole runs one invocation per lane
wherever the host's subgroup carries the wave. A wave the subgroup's width runs one lane an
invocation, as D884's ballot does. A wave of sixty-four on a subgroup of thirty-two runs two lanes
an invocation. The invocation's place in its subgroup, `p`, holds lanes `p` and `p + 32`. Every
vector instruction is emitted once for each of the two, from registers of its own. Lane reads,
permutes, DPP forms, `ds_bpermute`, whole masks, execution-mask writes, the local data share and
`s_barrier` all run this way. Only a scalar store or atomic still keeps the whole wave in one
invocation (D880), because every invocation computing the scalar state would make it.
`lane_exact::lane_masks` names the choice `Halves`.

**The wave and the subgroup.** The subgroup is the wave. A group of several waves is as many
subgroups (`SubgroupId`), and each wave's first thread is its subgroup's number times the wave's
width. The workgroup is the group's waves times thirty-two invocations. Every invocation computes
the scalar registers as its wave would (D880). Its wave's scalar control flow is then uniform within
its subgroup, and a group's waves keep theirs apart.

**Masks.** A comparison, a carry or the execution mask's seed makes each half of the mask by the
subgroup's ballot of that half's lane's condition: the low half from each invocation's first lane,
the high half from its second. A branch on `execz` or `vccz`, a saved mask and a lane count then
read what the wave holds, the same in every invocation.

**Another lane's value.** Lane `n` is invocation `n mod 32`'s first lane below thirty-two, and its
second from there. A read of lane `n` shuffles both halves by `n mod 32` and takes the one `n`
names. Each shuffle runs in every invocation, so a read is exact wherever each lane asks for a
different lane, as DPP and `ds_bpermute` do. `v_readlane`, `v_readfirstlane`, `v_writelane`,
`v_mbcnt`, the permutes and the DPP forms find each of an invocation's lanes at run time, as D760's
one lane does.

**The local data share and barriers.** The share is workgroup memory, which the group's waves share
as D892 has them. A wave's own program order between its lanes' accesses is a subgroup barrier after
each access. It is not the workgroup's, which the group's other waves would have to meet in control
flow they do not share. Byte writes and ors merge by atomics, as D760's do. `s_barrier` is the
workgroup's barrier between segments, where every invocation stands at the top level (D892).

**Scratch.** An invocation's array holds both its lanes' words, alternating: a lane's word `w` is
at `2 * (w / 64)`, plus one for the second lane.

**Kept as before.** An inactive lane stores nothing (D880). Escapes (D950), the loop budget and its
cut (D949), partial groups (D855) and device addressing (D964) are each computed per lane or per
invocation as in the one-lane model. A wave the subgroup does not carry, where the subgroup is not
known, still makes only its whole masks through workgroup memory in a group of one wave (D884).

**Why:** Grand Theft Auto V's full-screen ray traversal at `0x74005b7e8600` (2,171 instructions,
groups of 8 x 8, one wave of sixty-four) read its wave's lanes and masks, so it ran its whole wave
in one invocation. At `[240, 135, 1]` groups it lost the device at the 2 s watchdog in run 341.
Replayed alone (`a_captured_dispatch_replays_both_ways`), at a twentieth of the grid
(`[12, 135, 1]`), with both ways leaving the same output:

| | whole wave | two lanes an invocation |
|---|---|---|
| SPIR-V words | 643,542 | 586,702 |
| pipeline build (first run's host time) | 273 s | 4.2 s |
| device time, first run | 82.4 ms | 3.6 ms |
| device time, warm | 10.1-10.2 ms | 3.6-4.0 ms |

Over the whole grid, run two lanes an invocation, the dispatch takes 4.4-4.8 ms on the device. The
whole wave was not replayed at that size, because it loses the device. Both ways note an escape:
the capture holds only the guest pages the dispatch named, so some of what the run reached reads
zero here, and the times are the replay's rather than the title's.

**Rejected:**
- Each lane its own invocation, exchanging through workgroup memory as D884 does. Each lane read
  and mask costs two or three workgroup barriers. A barrier is the workgroup's, so the exchange
  serves a group of one wave alone. It must also never sit inside control flow that differs between
  the workgroup's invocations.
- A 64-wide subgroup asked for through subgroup size control: NVIDIA offers 32 only (D884).
- The two lanes as a loop of two passes over the run's instructions (D956). The module would be
  smaller, but the registers would be indexed by the pass, which spills an invocation's register
  file to memory. Two straight copies keep each lane's registers in the device's registers.
