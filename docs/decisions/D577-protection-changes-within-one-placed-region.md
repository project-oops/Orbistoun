# D577 - Protection changes within one placed region

**Status:** decided
**Date:** 2026-09-07

A guest protection change succeeds when its range lies wholly inside one region this process
placed for the guest: first the address space's own mappings, then the regions reported to the
kernel crate, such as the executable image and module pages. Containment is checked before
either authority acts, and a host refusal still answers `EINVAL`.

**Why:** a guest re-protecting its own module is ordinary, and the kernel crate is not the only
thing that maps guest memory. Requiring a single region keeps the original guard: a range
reaching into memory nobody placed, including the gap between two regions, is refused. Checking
first stops the address space filing a reservation failure for every successful module change.

**Rejected:**
- Only the address space's own mappings: refuses a guest's own module, which the guest checks.
- The union of regions: a range spanning two regions covers a gap nothing placed.
- Answering success on a host refusal: the call is worth making only if its answer is true.
