# D968 - A page committed after the device mapped its run is mapped before the next dispatch

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

The device maps a guest run once, at the first device-addressed dispatch that finds it (D964), and
leaves out every page of it that is not committed, private and read-write then. A page left out
is looked at again whenever the host pages' generation has moved. `orbistoun-mem` moves that
generation whenever a page is committed on touch or given another protection, which no readable
run's change shows (`mappings::pages_generation`). Before the next device-addressed dispatch the
backend finds the committed pages among the pieces it left out. It commits the rest of the 2 MiB
cell around each, releases the write-watch's guards there as it does for a new run, and maps the
cell's read-write pages. The table is then built again. A page the guest holds read-only stays
out, and what reaches it is still an escape.

A device-addressed dispatch whose module cut a loop (D949) and also reached memory no mapping holds
is now reported as a warning, not at debug level. A loop that polls a word nothing maps reads zero
for ever, so the escape, not the guest's loop, is the likelier cause of the cut.

**Why:** Grand Theft Auto V's compute program at `0x400003e18000` is a single-pass prefix scan with
decoupled look-back: three groups take tickets with `buffer_atomic_add`, publish
`{aggregate, inclusive, flag}` records of 20 bytes through a descriptor they compute, and group k
waits at `+0x49c`..`+0x4c8` on `buffer_load_dword ... idxen offset:16 glc dlc` until record k-1's
flag is nonzero. The records, and the element count the record descriptor's bound comes from, are
reached only by device address. In runs 336 to 338 the dispatch held the device until the watchdog
lost it. In run 339, with a different input from an earlier pass, its loop at `+0x4c8` ran past
1048576 iterations and was cut. The poll never saw a flag in any of them.

The first device-addressed dispatch of those runs mapped 6.9 GB of guest memory and left out about
46 GB. `orbistoun-mem` commits guest memory 64 KiB at a time as it is touched. The mapping's
generation moved only when a reservation came or went or a page's readability changed, so a page
committed after its run was first mapped was never mapped. Every access to such a page read zero
and stored nothing. That is an escape, reported only at debug level and hidden behind the loop cut.
GPU-only memory is first touched by the host when the command processor fills or binds it, after the
run was mapped. A scan's records there lose group k-1's flag store, and group k's poll reads zero
for ever. The capture holds only the bound buffers, so whether the records' page or the count's was
the one left out is not shown yet. The warning above names it on the next run that cuts the loop.

**Known limit.** A page that only device-addressed dispatches ever touch is never committed, so it
stays out, and every store to it is lost. Hardware backs all of it. The scan's records, its count
and its bound buffers share one 64 KiB chunk, which the host commits when it reads the bound
buffers, so the scan is not one of these. Amended by D971: the module now reports an escape's page,
and the host commits it so the next dispatch maps it.

**Rejected:**
- Mapping again whenever any run's page is committed, run by run: each commit would pin the run's
  committed gigabytes again, and the guest commits 64 KiB at a time.
- Mapping each newly committed piece alone: one import per 64 KiB touch runs into the driver's
  limit on allocations. The cell makes it one per 2 MiB touched.
- Relying on the commit D964 makes before it maps a run: the 46 GB were left out after that
  commit, and are mostly address space the guest reserved and never used. Committing it all would
  charge the process for it.
