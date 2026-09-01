# 2026-08-19 - Crunch continued: phases 2, 0e, and 3


**146 tests, gate green.** D054 and D055 added.

**Phase 2 (loading half).** Symbol databases load from disk with their own suffix, and
`SymbolDb::explains` measures a name list against real hashes - the self-verifying
loop from D025, where a collision *is* the proof. `orbistoun-cli verify` reports the
fraction of a module's imports a database can name. The generator half is still
outstanding and needs a suffix to test against.

**Phase 0e finished.** `orbistoun-cli report` surveys a module, persists a run report,
and prints the delta from the previous run of the same title. Retention purges on
startup. Verified end to end: first run has no diff, second run diffs against it, and a
different title correctly reads as a first run because identity is the content hash.

**Phase 3 done and verified on both platforms.** `orbistoun-cli load` reserves the span
a module demands: a 96 KiB module and a 96 MB commercial executable both place cleanly
at a supplied base.

**Surprises, all from running rather than reasoning.**
- **Per-segment reservation was wrong** (D054). Windows reserves at **64 KiB
  granularity**, so segments a few pages apart collided *with each other* - three of
  five "failing" on a module that fits fine. A module is one contiguous span. Real
  segment vaddrs are also not page-aligned (`0x147f0`), so the span rounds outwards.
- **`VirtualAlloc2` placeholders are unnecessary.** Plain `VirtualAlloc` at an explicit
  base already refuses rather than overwriting, which is the whole requirement.
- **The executable links at vaddr 0**, so it needs a placement base exactly as a module
  does. The attempt failed with "requested 0x0, kernel returned 0x134a2340000" - and
  that refusal is the design working, not a bug.
- **The Linux path was completely broken** (D055) and only running it in the multipass
  VM showed that: `MAP_PRIVATE` was missing, so every reservation returned `EINVAL`.
  Worse, *every* mmap error was mapped to `Conflict`, so it read as "range taken" and
  would have sent a reader hunting a phantom occupant. Wrong error messages are worse
  than none.
- **The rustdoc gate caught stale docs** - `orbistoun-mem` still described itself as
  unimplemented after being implemented. Cheap catch, and exactly why that lint is on.

**Next.** Phase 0b (ABI spike) to de-risk phase 4, then phase 4 itself.

