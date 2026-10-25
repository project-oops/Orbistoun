# D969 - A device-address lookup reads a table in device memory

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

This amends D964's lookup. It changes where the table lies, how a module reads it, and how a
module emits it. Guest bytes stay where D964 put them: host memory, imported.

**Table.** The table lies in device-local, host-visible memory where the device has such memory
with room for it. The host writes it through a mapping when the mappings change, and the device
only reads it. Where the device has no such memory, the table falls back to imported host memory,
as before. A slot is sixteen bytes, the chunk number and the delta, and a module reads it in one
load. A chunk's `PROBES` slots run consecutively from its home and never wrap: the table holds
`PROBES - 1` slots past its last home. One base address then serves all four loads, with no mask.

**Written marks.** The marks lie apart from the table, one word for each slot, in imported host
memory. The device only writes them, and each write is one the bus posts. The host scans them in
its own memory after the dispatch. A store, or an atomic, that lands nowhere marks a sink before
the first mark, and an atomic that lands nowhere is pointed at the same sink. The table is never
written on the device. The push block carries the marks' device address after the table's four
words (`DEVICE_TABLE_PUSH_WORDS` is 6).

**One helper function.** The lookup is one SPIR-V function, `DontInline`, that every access
calls. It is written whole among the module's helper functions (`Builder::into_helpers`) while
the function that calls it is still open. It takes the guest address and the table's address,
size, chunk shift and marks address as arguments, so it reads nothing of its caller's. It answers
the device address, zero where no mapping holds the address, and the device address of the mark.
Nothing in it assumes a lane loop: a module that runs one invocation per lane calls it the same
way. It answers both addresses as four 32-bit halves. Answered as a vector of two 64-bit words,
the same module, valid by `spirv-val`, lost the second word of a four-word store on an RTX 5070 Ti
(`every_word_of_a_wide_access_lands`); a hierarchy walk reads node pointers that way. The guarded
load and the store around it stay at the call site, since they are a few
instructions each. A store branches on whether the lane runs, which only the caller knows.

**Shared lookups.** An access's words share two lookups: its first word's and its last's. A chunk
is at least a page and an access at most sixty-four bytes, so its words lie in those two chunks
at most. Each word takes the first lookup where it shares the first word's chunk, and the last
lookup otherwise. A scalar load of sixteen words calls the helper twice, not sixteen times.
64-bit constants are declared once per module (`Builder::declare_constant`).

**Measured** (RTX 5070 Ti, `device_addressed_access_cost`). The test times 128 groups of 64
lanes, each running a dependent chain of 4 and then 36 global loads, optionally each beside a
store. It takes the difference over the 32 extra steps.

| | whole wave, load | whole wave, load + store | per lane, load | per lane, load + store |
|---|---|---|---|---|
| D964 table, data in the cache | 6,822 ns a step | 297,538 | 256 | 36,797 |
| this table, data in the cache | 6,819 | 206,503 | below resolution | 24,356 |
| data past the cache, table in host memory | 46,791 | 263,600 | 13,044 | 32,384 |
| data past the cache, this table | 40,128 | 168,416 | 11,600 | 32,784 |
| data past the cache, this table, no marks | 39,762 | 59,256 | 11,847 | 30,503 |
| data past the cache, this table, one helper | 41,031 | 358,606 | 11,431 | 35,856 |

"Data past the cache" means each step reaches 2 MiB further, 72 MiB in all, so the device's
last-level cache does not hold the chain. Per access over every lane, a load costs 1.4 ns per
lane and a load beside a store about 4.4 ns. That is the bus's throughput for scattered 4-byte
accesses to host memory. The table's placement saves 10 - 14 % of a load. The marks are most of a
whole-wave store's cost and little of a per-lane store's. The helper costs a whole-wave store
twice what the inlined lookup did, and costs nothing measurable per lane.

Grand Theft Auto V's traversal program at `0x74005b7e8600` was translated device-addressed
offline (`listed_programs_translate_device_addressed`, 4 user words, 3 group ids, 3 thread id
components, 4096 scratch bytes a wave). It came out at 9,492,994 words with the lookup inlined at
every access of every lane, and at 681,822 words with the helper. Run 340's driver had not
finished compiling the inlined module after five minutes.

**Guest bytes stay imported; copies complement them (D974).** D974 builds the copies this
section admits. It also measures the open question below: a page imported and then made read-only
is still read by the device, but a device store into it lost the device. So no guard stands over
an imported page, and the copies are kept coherent by the host's write-watch record instead. D974
also compares the technique with a mirror refresh that compares guest bytes with a shadow copy.

**Guest bytes stay imported; mirrors complement them later (as first decided).** Once the table lies in device
memory, what remains is guest bytes crossing the bus. A hierarchy walk over gigabytes, with each
lane's node reads scattered, is bound by that throughput, not by the lookup. A device-local mirror
of a hot range would remove it. The table already supports one per chunk, by pointing the chunk's
delta at the mirror, with no change to the modules. A mirror is admitted only under all of these
conditions:

- The imported pages stay every guest byte's home. A mirror is a copy that the table names for one
  dispatch at a time.
- The CPU cannot write a mirrored chunk unseen. A write-watch guard holds it read-only from the
  CPU (D824), so a CPU write faults, drops the mirror and lands in the home pages. The host
  uploads a mirror only for chunks unwritten since its last upload. Whether a page imported into
  the device may be protected afterwards without disturbing the import is not yet measured; D964
  releases guards before importing because the driver needs writable pages while it imports.
- What the device writes in a mirror, by the marks, is copied back to the home pages before the
  dispatch is reported done. The command processor and the CPU then read it there, as they do now.
- A dispatch that may hand off to the CPU while it runs reads home pages, not a mirror. Such a
  dispatch either polls a word the CPU writes or writes a word the CPU polls. A mirror makes a
  dispatch's view of a chunk the host's view as of the dispatch's start. The single-pass scan that
  D964's volatile loads serve polls between workgroups, which a mirror keeps, since both workgroups
  read the same mirror.

**Rejected:**
- Replacing imported memory with device-local copies throughout. During a dispatch it loses the
  coherence the import gives for free: a CPU write is not seen until the next upload, and a device
  write is not seen until a copy-back. It also needs every CPU write tracked, at a page guard's
  granularity, over every mapping.
- Inlining the lookup at every access. That gave 9.5 M words for one program, and the driver did
  not finish compiling it.
- A sorted range table searched by halving. It costs a dependent load per step. The table's probes
  are one round of independent loads from one base.
- A flat page table indexed by chunk. At 64 KiB chunks over a 47-bit space it is gigabytes; two
  levels make two dependent loads.
- Marks in device memory with a summary, read back through the host mapping. That avoids the bus
  writes that dominate a whole-wave store. But the host's read of device memory through a mapping
  is uncached, and it is not measured. Revisit it once whole-wave dispatches are gone. They are
  being moved to one invocation per lane, where the marks cost little.
