# D980 - A device-addressed capture holds every chunk its dispatch reached

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

Amends D971's known limit. A device-addressed dispatch (D964) that `ORBISTOUN_CAPTURE_DISPATCH`
asks for is captured after it runs, not before. The capture holds every guest chunk its lookups
found, as it was before the dispatch, and a replay reaches what the run reached: a hierarchy's
lower levels and the nodes its pointers name, not only the pages the user data names.

**What is kept.** The dispatch is asked whether it is captured before its table is placed. A
captured dispatch records what it touches, with D974's touched marks, whether or not this is one of
its program's recording dispatches. After it runs, while the mappings' lock is still held, the
capture takes the chunks it touched and the chunks its stores marked. It keeps the 64 KiB guest page
around each one, as `guest-<n>.bin`. The pages its user data names are still read before it runs
(D971) and laid over the rest. A page's bytes outside every chunk read zero; no lookup reached them.

**Before, exactly.** A chunk the dispatch only read holds after it what it held before. A chunk it
stored into is read from its home pages before the copy-back. A copied chunk's stores land in its
copy (D974), so its home pages still hold the bytes from before. A chunk the dispatch stored into
that was not copied has lost its bytes from before, unless a named page holds them. Then the
capture is not written. Every chunk the dispatch reached is copied for the program's next dispatch,
the chunks it stored into among them, and the capture waits for that dispatch. A copied chunk's
stores are copied back after the dispatch as any are. At most four dispatches are tried. Past them,
or on a thread that does not use the copies, the capture is written with those chunks as the
dispatch left them, `guest-state after`, and a warning says how many.

**The test.** `a_replayed_device_addressed_capture_walks_the_pointers_the_run_did` follows the
user data's pointer to a pointer, then to a table of counters, and adds one to each lane's counter.
Captured as D971 kept it, the replay read zero past the first pointer, escaped, and left another
output. Captured as here, the first dispatch stores into a chunk not copied, so the second is
captured. Its replay leaves the hash the run's own output has, twice, with no escape.

**Rejected:**
- Running the dispatch again into a scratch copy of what it reached: the scratch would have to hold
  every chunk before the dispatch names them, which is what is being found out.
- Keeping the pages after the dispatch, as a loss's capture does: a replay would start from what
  the dispatch left and store into it again.
