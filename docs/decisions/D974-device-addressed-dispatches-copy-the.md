# D974 - Device-addressed dispatches copy the guest chunks they reach into device memory

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

A device-addressed dispatch (D964) reaches the guest chunks it was measured to reach in copies in
device-local memory. The table points each copied chunk at its copy (D969), so the modules do not
change. The imported pages stay every guest byte's home, and a copy is clean between dispatches.

**What is copied.** What is copied is measured, not guessed. A program's first dispatch records
the chunks its lookups found, and so does every sixteenth after it. Each lookup in such a dispatch
sets its chunk's touched mark, a word beside the written marks, behind a branch on one value for
the whole dispatch. A chunk recorded touched is copied from the next dispatch on, while the copies'
memory lasts. That memory is one device-local buffer of 1 GiB, or the largest the device grants
down to 64 MiB. When it is full, the copy least recently found touched is given up first. A chunk
is never copied if no import holds it, if the CPU wrote it while a dispatch read its copy, or if
the host's write-watch cannot watch it.

**Coherence.**

- *CPU writes.* Before each dispatch, each run of copied chunks is marked in the host's
  write-watch (D824). Every chunk of the run written since its last mark is then copied in again
  from its home pages, on the device, before the dispatch is submitted. The mark comes before the
  copy, so a write after the mark is the next dispatch's to find. The watch is the host's own
  record (`GetWriteWatch`), never a guard (see below).
- *Device writes.* After the dispatch, every copied chunk its stores marked (D969) is copied back
  to its home pages, on the device, before the marks reach the command processor, and given up.
  The command processor then treats those chunks as any device write (D964).
- *Per thread.* A thread may turn the copies off for its own dispatches (`set_copying`). They then
  run over a table that points at no copy, and the copies are left as they were for every other
  thread. Turning the copies off is never process-wide, so a test comparing the two disturbs none
  beside it.
- *Hand-off.* Before the copy-back, each run is asked whether the CPU wrote it while the dispatch
  ran. A chunk the CPU wrote then is one the CPU and the device share while a dispatch runs, as in a
  poll or a hand-off. It is dropped and refused for good. Where the device also stored into it, the
  copy-back lands last and a warning says so: D964's known limit for a bound buffer, at a chunk's
  grain.
- *Failure.* A dispatch that fails has its copies dropped, not copied back.

**One walk of the write-watch a dispatch.** All the copied runs are marked together
(`orbistoun_mem::watch::mark_all`), and the hand-off check after the dispatch harvests them
together (`harvest_all`). Each chunk is then answered from the record (`recorded_since`). Each
reservation the runs lie in is walked once, from the first run in it to the end of the last. The
guest's record of its reservations (`mappings::reservation_of`) says which reservation that is,
not the host.

This replaced one mark per run. In a 480 s GTA V run, those marks took 4 - 8 ms each over
250 - 275 runs: 0.8 - 2.1 s before every device-addressed dispatch, 132 of the 168 s spent on the
device. Measured with `write_watch_cost_over_an_import`, over 256 one-page runs 4 MiB apart in a
1 GiB reservation:

| | each run marked alone | one mark of the whole reservation | the runs marked together |
|---|---|---|---|
| not imported | 64 - 76 ms (0.25 - 0.30 ms a run) | 0.9 - 1.2 ms | 0.74 - 0.87 ms |
| imported | 234 - 256 ms (0.9 - 1.0 ms a run) | 150 ms | 0.84 - 0.95 ms |

The cost of a mark is not the host's `GetWriteWatch` but its `VirtualQuery`. The harvest asked it
for each run's region, and its cost grows with the regions of the whole allocation: 0.25 ms in a
gigabyte reservation, 0.9 ms once the device mapped it, 4 - 8 ms in GTA V's. A mark of the whole
reservation pays instead for reporting every page written in it. With one walk, the benchmark's
short chain with copies runs as fast as without them (0.17 ms against 0.27 ms, whole wave; 0.12 ms
both, per lane): the per-dispatch cost measured below is gone.

**Imported pages cannot be guarded.** Measured on an RTX 5070 Ti
(`an_import_s_read_only_pages_are_still_read`). The test makes pages read-only after their import,
as a D824 guard does. The device still reads them. But the same program storing into them lost
the device, and every later submission of the test process failed. A guard over a page the device
maps is therefore never set: an import forbids guards over its range while it lives
(`orbistoun_mem::watch::forbid_guards`). A guard standing there is released and its pages noted
written, and a mark there answers from the host's write-watch alone. This also closes a gap in
D964 as it stood. D964 released guards only when it imported a run. A texture or draw buffer marked
later over an imported run could then guard pages the next device-addressed store reached.

**Measured** (RTX 5070 Ti, `device_addressed_access_cost`, D969's benchmark past the cache). The
benchmark is a dependent chain over 72 MiB, ns a chained step, beside the other tests of the
file:

| | whole wave, load | whole wave, load + store | per lane, load | per lane, load + store |
|---|---|---|---|---|
| home pages | 41,125 - 42,500 | 145,719 - 147,566 | 10,506 - 10,747 | 29,750 - 30,988 |
| copied | 21,797 - 22,038 | 157,112 | 206 - 3,619 | 21,812 |
| copied, a stored chunk kept copied | 21,797 | 179,512 | 206 | 77,578 |

A load from a copy costs a twentieth to a half of one from the home pages, per lane, and half
when the whole wave is one invocation, whose lanes run one after another. A store into a chunk
kept copied was worse than no copy: the chunk was copied back whole after every dispatch, 64 KiB
for each lane's scattered word. So a chunk a dispatch stores into is copied back and given up,
and only a chunk a recording dispatch read alone is copied. Each dispatch with copies pays
0.2 - 0.36 ms for the write-watch's walk of the copied runs and the copies' submissions, measured
as the short chain's time with and without them.

**The other mirror refresh.** A refresh found elsewhere keeps a host-side shadow of each mirrored
allocation's bytes and compares it with guest memory, 64 KiB at a time, before each use. It copies
a changed block into a host-visible device buffer through a mapping, on several host threads. For
allocations it cannot compare, it uses a per-block generation from its memory tracker. It mirrors
whole allocations, by kind, within a budget, and writes back by comparing too. This decision
differs on four points:

- It asks the host's write-watch which chunks changed instead of comparing. That holds no second
  copy in host memory, and reads none of the guest's bytes when nothing changed.
- It copies on the device, by DMA from the imported pages, instead of the host writing across the
  bus through a mapping.
- It copies back only the chunks the device's own stores marked, instead of comparing to find
  them.
- It copies the chunks a recording dispatch reached, not whole allocations, so a hierarchy over
  gigabytes takes only the part a pass walks.

The comparison costs host time in proportion to the mirrored bytes at every use, and catches a
write no watch reports. The write-watch costs a kernel walk of the copied runs at every dispatch,
and sees every write the host's page tables record. A device or DMA write into guest memory is not
among them, which is why the marks exist (D964).

**Rejected:**
- Guarding the copied chunks' home pages with D824's read-only guards: a device store into a
  guarded page lost the device.
- Copying every mapping whole: a run of gigabytes, most of which no pass reads, against a
  budget of device memory.
- Writing the copies through a host-visible mapping: the host's writes cross the bus one store at
  a time, where a device copy moves a chunk in one transfer.
- Deciding hotness by allocation kind or size: an assumption about the title's layout, where
  the passes' own lookups say what they reach.
