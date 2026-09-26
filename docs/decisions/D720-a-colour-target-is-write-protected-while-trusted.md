# D720 - A colour target is write-protected while trusted unchanged

**Status:** assumed
**Date:** 2026-09-26

When the command processor has read or written a colour target, the target's pages become
read-only; a write to them faults, and the handler records the target as written, restores the
protection and retries. A target still read-only and unwritten is unchanged; one written, or
never protected, is compared against the kept bytes.

**Why:** the host write-watches only memory it allocated privately, and guest direct memory is
mapped views, so without protection every submission compares 8 MB against the kept bytes. Page
protection sees every write, as write-watch does, and errs the same way: a write of identical
bytes is caught and then passes the comparison. The command processor's own writes, and the
other page guards, release the protection first rather than faulting, and one target is
protected at a time.

**Rejected:**
- Comparing the whole target before every submission: exact, and the dominant cost of a frame.
- Write-watch on guest direct memory: the host does not offer it for mapped views.
