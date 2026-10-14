# D883 - A dispatch's global loads are bound from the addresses its waves form

**Status:** decided
**Date:** 2026-10-14

A guest dispatch's global loads with no scalar base (`global_load_dword*` and the sub-word loads,
D878) are traced as one buffer after the dispatch's other buffers, as a primitive shader's are
(D758). The buffer is the range the loads' lanes read: from the word the lowest address is in to the
end of the widest read from the highest.

The addresses come from the program through the last load, run on the host for every wave of every
group the dispatch runs (D858), through the translator's own semantics. Each run starts as the wave
does on the hardware: the user data from `s0`, then the workgroup id components `COMPUTE_PGM_RSRC2`
enables, and in each lane the thread id components `TIDIG_COMP_CNT` asks for, the wave's first
thread plus the lane, x fastest. A wave's execution mask holds the lanes that have a thread; the
last group of a partial dispatch (D855) runs only the threads it has. Every other vector register is
zero. A branch whose condition is known is followed.

The module reads the range relative to its guest base, the address's low half, which the dispatch
puts in its push block after the partial-group words. A load whose lanes' addresses or mask the
host cannot know is refused by name, and so is a dispatch of more waves than a dispatch runs
through the host in one preparation.

**Why:** Grand Theft Auto V's first compute program loads bytes from addresses each lane forms,
`global_load_ubyte v0, v[1:2], off`. A guest dispatch writes guest memory for real, so an access
it cannot place exactly is refused rather than read through the window (D746), and the program
stopped there. A global load has no descriptor, so nothing but the addresses the lanes form bounds
it - the reason D758 gives for a primitive shader, which holds for a dispatch in the same way.

**Rejected:**
- Reading a global load through the dispatch's window: the window sits where the program's
  descriptors are, and a global address can be anywhere in the guest's mapping.
- Binding the whole mapping from the address's high half: it is the guest's arena, far more than a
  dispatch reads, and D758 rejected it for the same reason.
- One run for the dispatch with the group ids unknown: every group then forms unknown addresses,
  and the load is refused.
