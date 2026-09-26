# D584 - Guest-visible blocks come from one fixed region

**Status:** decided
**Date:** 2026-09-26

Every handle or opaque block the guest is given comes from `blocks::block`, which bump-allocates
aligned, zeroed, never-freed blocks from `GUEST_BLOCK_BASE` in `orbistoun-mem`. Block n is the
same address in every run; exhaustion falls back to the host heap and `blocks::repeat` reports it.

**Why:** a guest reads fields through a handle, so a handle must be a real zeroed block, but
nothing requires the host allocator to choose where. Host heap addresses differ every run and
make two runs incomparable. Guest-visible memory is what `orbistoun-mem` is for, and every
subsystem that issues handles already sits above it on the dependency spine.

**Rejected:**
- `Box::leak` per site: a different address every run, and each site is fixed separately.
- The allocator in `orbistoun-kernel`: the file system is a sibling and cannot reach it.
- Installing the allocator through a hook: indirection where a downward dependency suffices.
- Failing on exhaustion: a run that loses repeatable handles still has evidence worth keeping.
