# 2026-08-19 - Terminology and documentation pass


**Done.** Vendor trademarks removed from prose and from our own API surface per D015
(`SceError` → `GuestError`, `sce_module!` → `guest_module!`,
`SCE_SEGMENT_RANGE` → `VENDOR_SEGMENT_RANGE`). ABI identifier strings kept, since the
NID is computed from them.

Roadmap restructured to phases 0, 0b, 1-6 (D020, D021, D022). `PROJECT_STATUS.md`
gained "Declared but not yet consumed" and "Never executed" sections; `BACKLOG.md`
gained a Housekeeping section. This file and `DECISIONS.md` created, with all
prior decisions backfilled as D001-D023.

**Surprises.**
- Substituting terms across prose broke four sentences into nonsense ("Not a the
  previous-generation console emulator"). Blanket `sed` over prose needs a grammar
  pass afterwards - check for doubled articles.
- Roadmap renumbering was free: old steps 1-6 map exactly onto new phases 1-6, so no
  cross-reference broke. Only additions were at the front.

**Next.** Unchanged - phase 0.

