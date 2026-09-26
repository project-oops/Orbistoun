# D720 - A colour target is write-protected while it is trusted unchanged

**Status:** assumed
**Date:** 2026-09-26

When the command processor has read or written a colour target, the target's pages become
read-only; a write to them faults, and the fault handler records the target as written, restores
the protection and retries the write. A target still read-only and unwritten is unchanged; one
written, or never protected, is compared against the kept bytes.

**Why:** the host write-watches only memory it allocated privately, and guest direct memory is
mapped views, so without protection every submission compares the target's 8 MB against the kept
bytes. Page protection sees every write to the pages, as write-watch does, and errs in the same
direction: a write of the same bytes is caught as a write and then passes the comparison. The
command processor's own writes, and anything that guards the pages another way (D717, D719), release
the protection first rather than faulting. One target is protected at a time: the one the kept
bytes belong to.

**Rejected:** comparing the whole target before every submission - exact, and the dominant cost of
a frame.
**Rejected:** write-watch on guest direct memory - the host does not offer it for mapped views.
