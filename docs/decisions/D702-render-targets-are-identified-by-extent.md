# D702 - Render targets are identified by extent

**Status:** decided
**Date:** 2026-09-16

A colour render target's `ResourceId` is its width and height tagged with the top bit of the id
space, not its base address.

**Why:** the size register is corroborated by a hardware capture and a constructed stream, while
the two disagree on the base register's offset, and a wrong base yields a plausible wrong id.
Same-sized targets share an id, which holds while the backend keeps one frame per extent; once
it keeps host objects per address, the id must key on the address. The top-bit namespace keeps
target ids disjoint from sequential shader ids with no shared counter.

**Rejected:**
- Keying on the base address: rests identity on a disputed register.
- A shared sequential counter: a fresh id per submit re-records the same target every frame.
