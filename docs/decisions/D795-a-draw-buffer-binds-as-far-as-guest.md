# D795 - A draw buffer binds as far as guest memory holds it

**Status:** decided
**Date:** 2026-10-11

A draw buffer whose range, as its descriptor's bounds rules admit it (D733), runs past the end of
the guest memory that holds it binds the part that memory holds: the longest mapped run of whole
words from its base. A range with nothing mapped at its base is still refused.

Assumed (`known_by = assumed`): that a title which runs on the hardware never reads the part past
its mapping. A fetch from unmapped memory there is a GPU page fault, not a value; the bounds rules
decide which records an access may index, not which a draw does. So the bytes left unbound are
bytes no working draw reads, and a read that did reach them would, here, read the host's
out-of-range answer where the hardware would have faulted.

**Why:** mesa-cube's vertex buffer starts eight bytes into a four-mebibyte buffer and its
descriptor admits four mebibytes of sixteen-byte records from there, eight bytes past the
buffer's mapping. The draw was refused, the context failed to retire, and Mesa aborted.

**Rejected:**
- Bounding each buffer by the records the draw indexes: right, and the better end state, but it
  needs the index each fetch uses traced through the shader, which the translation does not yet
  report.
- Reading zeros for the unmapped tail: a fault is not a zero, and zeros would be bound as though
  the guest had written them.
