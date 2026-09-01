# D054 - A module is reserved as one contiguous span, not per segment

**decided** · 2026-08-19 · corrected by running against real material

Phase 3 was first implemented as one reservation per loadable segment. That is wrong,
and two facts observed on real material force the correction:

- **Windows reserves at 64 KiB granularity**, not page granularity. Segments a few
  pages apart share a 64 KiB block, so per-segment reservation makes neighbouring
  segments of the *same module* collide with each other. That produces a conflict that
  says nothing about whether the address is available - it is entirely self-inflicted,
  and it was reported as three of five segments "failing" on a module that fits fine.
- **Segment addresses are not page-aligned.** A real module carries a `p_vaddr` of
  `0x147f0`. A mapping must begin on a page boundary, so the span is rounded outwards
  at both ends rather than the segment being rejected as misaligned.

So the span from the lowest segment start to the highest segment end is reserved once,
page-aligned outwards. Per-segment protection is applied when segments are populated,
which is the loader's job rather than a placement survey's.

### `VirtualAlloc`, not `VirtualAlloc2` placeholders

Earlier planning assumed placeholders would be needed. They solve a different problem -
reserving a large region and later splitting it - and plain `VirtualAlloc` at an
explicit base already has the required semantics: it never overwrites an existing
reservation and returns null when the range is taken. Placeholders become worth
reaching for if sub-dividing a reservation becomes necessary; before then they are
complexity with no payoff.

### Executables link at address zero

The commercial executable's segments start at `vaddr 0`, so it needs a placement base
exactly as a module does. Attempting to honour address zero fails - the null page is
never mappable - and **the failure is the design working**: the kernel offered a
different address and the reservation was refused rather than silently relocated
(`requested 0x0, kernel returned 0x134a2340000`). A guest that asked for an address
and silently got another corrupts itself in ways that look like anything else.

Verified: the 96 MB executable places cleanly at a supplied base, as does a 96 KiB
module.

