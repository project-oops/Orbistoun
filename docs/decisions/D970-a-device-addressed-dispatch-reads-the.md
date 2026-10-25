# D970 - A device-addressed dispatch reads the descriptors it loads where they lie

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A device-addressed dispatch (D964) reads every scalar load no bound buffer serves where it lies,
by device address. That includes a load of eight or sixteen words that an image instruction's
descriptor is traced to. The image instruction still binds the image the host read from that table
load, by the load that reaches it (D895, D934, D963), not by the registers' values. The registers
also hold the guest's words, so the program may read them as values.

**Bound tables.** A dispatch that is not device-addressed reads such a load's words too, wherever
the buffer tracer bound its table (D733, D746). The tracer serves a load from a traced table base
as an access to that base, so the bound buffer holds the descriptor's words, the same bytes the host
bound the image from. Reading them costs one bound-buffer read a word, and nothing new reaches the
module. Only a load with no buffer is still taken as the image's name alone. Its registers then hold
nothing, and a read of them as a value is refused.

**Along the program's paths.** Whether a read is of such a name is decided over the program's
blocks, not in the order the translation emits them. As it translates, the module records each
instruction's registers: those a load leaves holding a name, those it writes, and those it reads as
values. A forward dataflow over the blocks then finds what may hold a name entering each one,
starting from nothing at the entry and joining at merges by union, to a fixed point. A read is
refused, at its own offset, where a name may reach it with no write between on some path. A load on
another path never refuses a read, and a write on another path never clears a name, however the
blocks are laid out.

**Control flow.** Nothing of a device-addressed dispatch runs on the host. Its module carries the
wave's control flow itself, with one program counter per wave (D098). A branch on the execution
mask, on the condition mask or on the scalar condition tests the whole wave's value on the device.
The lanes the mask turns off keep their registers and memory as they were. A loop whose lanes leave
it after different counts runs until the mask empties, as the hardware's does. D758's refusal of a
branch on a mask the host cannot know is the host trace's. Where device addressing is offered, it
only sends the dispatch to be prepared device-addressed.

**Remembered.** A program the host trace refused for what it cannot know, and which was then
prepared device-addressed, is remembered by its address and its first 256 bytes. Its later
dispatches are prepared device-addressed from the start. Otherwise the trace would run every group
and wave of each dispatch only to end in the same refusal (D900, D914). Other bytes at the same
address are another program, traced again.

**Why:** Grand Theft Auto V's ray-traversal passes were refused device-addressed. The refusal
read "a dispatch reads an image descriptor it loaded from its table as a value". The program at
`0x74005b7e8600` samples an image through the eight words at `+0x160` of its table, then tests the
sixth of them as a value (`s_cmp_lg_u32 0, s29` at `+0x10b4`). The program at `0x74005b854a00`
loads an instance record with `s_load_dwordx16 s[8:23]` from an address it computes. Words 8 to 11
are the bottom level's hierarchy descriptor, and it tests the last word as a value (`s_and_b32
vcc_lo, s23, 0x7f000000` at `+0x1158`). Other loads of image descriptors into `s[8:15]` made the
whole record look like two image descriptors. Read where they lie, both programs translate with
every instruction they hold, and so does the third pass at `0x74005b857200`. The run's log joined
the host trace's D758 refusal to the device preparation's own, an instruction not yet translated.
That made it look as if the device path had been refused for its control flow.

Run 342 refused the same way for the wave-32 full-screen pass at `0x74005b809c00`, which is not
device-addressed. It loads `s[16:23]` from its table at `+0x120` on one path and writes a compare's
mask into `s20` on the other (`v_cmp_ne_u32_sdwa s20` at `+0x1a4`, read at `+0x2a4`). Which
registers hold only a name was tracked in the order the translation emits blocks, not along the
program's paths, so the read on one path was taken for a read of the load on the other. Read
through its bound table, the load leaves nothing to track, and the program translates. Tracked in
emission order, a layout can also accept a read a name reaches: a branch's taken block that writes
the register is emitted between the load and the read, though the way that falls through jumps
from the load straight to the read. The dataflow refuses that read.

**Rejected:**
- Judging per load rather than per register group whether a load reaches an image: it frees the
  instance record, but the first program really samples through the words it reads.
- Running the program on the host to find which descriptor words it reads as values: the host
  trace D964 replaces for these dispatches.
- Binding the table's range as a buffer for a device-addressed dispatch so the words could be read:
  its extent is unknown before the program runs, which is why the dispatch is device-addressed.
- Handing the host's descriptor words to the module in the push block or a buffer of their own:
  the bound table already holds them.
- Sending a dispatch refused for a value read to device-addressed preparation: device addressing
  is for what the host cannot know, and the host knows this table.
- Tracking names in emission order: any layout change, the structured one's included, can make it
  refuse a read the load never reaches or accept one it does.
- Deciding the names from the instructions alone before translating: it restates the
  translation's choice of which loads are names, and the two could drift.
