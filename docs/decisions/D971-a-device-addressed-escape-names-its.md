# D971 - A device-addressed escape names its page, and capture keeps the pages a dispatch names

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

**The escape's page.** A device-addressed module (D964) records the guest page of each access no
mapping holds. A page is 64 KiB, bits 47:16 of the address the access formed. Each invocation keeps
the highest such page in a private word. When the dispatch publishes its escape flag, it raises
observation word `DISPATCH_ESCAPE_PAGE_WORD` (2) to that page by an atomic maximum, so invocations
that escaped at different pages leave one whole page. The host reads it back as
`GuestDispatch::escaped_page`. An escape with no address, such as a descriptor whose addressing is
not modelled, sets only the flag, as before.

After the dispatch, the backend looks for the page among the pieces of runs it left out (D968). If a
piece holds it, the backend commits the 2 MiB cell around it, cut to the piece. That moves the host
pages' generation, so the next device-addressed dispatch maps the cell. A page outside every mapped
run is only reported. The dispatch is not run again. What it stored and combined through mapped
pages has already landed in guest memory, and a second run would apply its atomics twice. So the
dispatch that escaped stays as it ran, and the next one finds the page mapped. A page that only
device stores touch is therefore mapped after the first dispatch that reaches it, where before it
was never mapped.

**Capture and replay.** A device-addressed dispatch reaches guest memory in place, and that memory
is not among its bound buffers. A capture of one therefore keeps the guest pages its user data
names. For each word of the push block and the next, it takes bits 47:0 of the address they form,
then that 64 KiB page and the one after it, wherever a mapping holds them. The device table's push
words are left out. The pages are written as `guest-<n>.bin` at their guest addresses. A
`guest-state` line says whether they were read before the dispatch ran or after. They are read
before where a capture may be written of it: a program `ORBISTOUN_CAPTURE_DISPATCH` names, or any
dispatch while `ORBISTOUN_CAPTURE_RECENT` keeps the latest. A dispatch written only because it lost
the device keeps them as it left them, since nothing was copied before it ran.

`replay-dispatch` reserves each captured range again at its own guest address, writes it before
each run, and runs the module through the device-address mappings as a run does. A replay is not
captured again. `--out` writes the ranges as the run left them, as `guest-<n>.bin`, and the output
hash folds them in.

**Known limit.** Memory a dispatch reaches only through pointers it reads from memory is not kept,
for example a hierarchy's lower levels named by its top level. In a replay it reads zero and is
reported as an escape, with its page named. Amended by D980: a capture asked for by program is
written after its dispatch, with every chunk the dispatch's lookups found, as it was before.

**Why:** Grand Theft Auto V's prefix scan at `0x400003e18000` (D968) waits on flags that it reaches
only by device address. A page that only device stores touch was never committed, so it was never
mapped: every store to it was lost and every load read zero. The escape flag alone did not say
where. The hang could not be reproduced offline either: `replay-dispatch` ran a device-addressed
module through the bound-buffer path, which names no device table, and the capture held none of the
memory the scan reads and writes.

**Rejected:**
- Running the escaped dispatch again once the page is mapped: its stores to mapped pages and its
  atomics have already landed, and a second run would apply them twice.
- Committing every page of every run before mapping it: see D968.
- Marking every page a dispatch reads, for capture: a store per load in every device-addressed
  module, to serve a diagnostic.
