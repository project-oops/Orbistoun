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

**Amended 2026-09-28:** the condition above came true with D714, which keeps each target's
frame on the device until its flip. A double-buffered title draws alternate frames into two
same-sized targets, and one id for both handed one buffer's frame to the other (Bugdom's menu
kept only its last submission's draws). The id now keys on the base address as well as the
extent, and on the extent alone for a stream that sets no base; the D714 write-back already trusts
that address.

**Rejected:**
- Keying on the base address: rests identity on a disputed register.
- A shared sequential counter: a fresh id per submit re-records the same target every frame.
