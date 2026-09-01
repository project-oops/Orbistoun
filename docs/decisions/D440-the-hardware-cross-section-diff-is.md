# D440 - The hardware cross-section diff is mined out for clean fixes


**measured** - 2026-09-01 (user-directed, /loop)

Re-ran the obSCEne probe under orbistoun and diffed all shared `res` values against the hardware report
again. Every clean, system-wide value-swap this diff surfaced is now applied and verified byte/value-exact
(D437 the ReservedLow map floor; D438/D439 the error codes - munmap/event-flag/close/audio/pad/open/read/
write/lseek; the GNM dispatch shader-type header). What remains divergent is, in every case, *not* a clean
fix:

- **Correctly left alone** - per-run (`010-kernel/*` timing, `018-relational/*`, `030-thread/self`,
  `050-time/usleep`), per-account (`070-user/initial-user`, `100-input/open`), and per-process
  (`020-memory/flexible-available`, the probe's own budget, D437).
- **Needs more obSCEne data** - `015-sync/mutexattr-round-trip` reports the *count* of types that
  round-trip (4 vs orbistoun's 5) but not *which* of the five hardware rejects, so matching it would be
  guessing the normalisation; and `150-memory-map/{walk,after-allocation}` is the region-count face of the
  still-open map-shape question (D083). A per-type mutexattr probe and a map-region dump would settle them.
- **Needs a design or model decision, not an implementation** - `060-module/dlsym-rejects-bad-handle`
  wants handle validation orbistoun's global resolver does not do; `060-module/sysmodule-query` and
  `110-modules/*` turn on whether a statically-linked module counts as "loaded"; `005-generation/neo-mode`
  is a base-vs-neo machine choice that carries real regression risk (a guest told it is neo may then
  expect capabilities orbistoun does not provide). These are flagged rather than decided.
- **Deliberate** - `165-gnm/dispatch-direct`'s sixth dword is surrounding hardware state orbistoun does
  not model (its packet header now matches, D439).

So the "implement from the data on disk" pass is complete: the remaining tail needs either a new
measurement or a decision, which is the loop's stop condition reached honestly.

