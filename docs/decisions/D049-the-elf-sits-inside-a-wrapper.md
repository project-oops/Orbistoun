# D049 - The ELF sits inside a wrapper

**Status:** decided
**Date:** 2026-09-26

A container is a wrapper around an inner ELF, and the inner ELF's offset is read from the
wrapper header, never assumed. Both wrapper generations parse through the same reader, and the
generation read is reported.

**Why:** real containers do not begin with an ELF header, and an offset that held on every file
inspected is still an observation, not a specification. The two generations differ only in
magic and version, so a second parser would duplicate the first; a title built for the
previous generation is a different emulation problem, so the report names it.

**Rejected:**
- Expecting an ELF at offset zero: rejects every real container.
- Hardcoding the observed offset: a hypothesis presented as a constant.
- Refusing the previous generation: loses titles that parse correctly.
