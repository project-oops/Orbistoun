# 2026-09-02 - (/loop) The last clearly-standard stubs: vsprintf_s, SetVirtualRangeName

Cleared the two remaining unambiguous unimplemented functions from the corpus's flagged list.

- **`vsprintf_s`** (libc, PPSA21564): Annex K's bounds-checked `vsprintf`. Its observable behaviour -
  render the format against the argument list into a buffer bounded by the count, always terminated,
  answer the length - is identical to `vsnprintf` for a valid buffer, so it delegates to it. Published,
  knowledge recorded.
- **`sceKernelSetVirtualRangeName`** (kernel, PPSA21564): attaches a debug name to a virtual range.
  Accepted and answered `OK` with nothing stored, because the name is advisory - it labels a range for
  host profiling/crash tools and no guest-readable interface hands it back, so the guest cannot observe
  whether it was kept. The same honest shape as `sceKernelMunmap` returning `OK` without tearing a
  reservation down (D273); a null name or zero-length range is the one thing refused. Not accept-and-lie,
  because there is nothing observable to lie about.

clippy/fmt/tests/knowledge-audit clean across kernel and libc; no regression (PPSA21564 500,257 calls
unchanged). Neither moves a wall - both are surface reduction, the guest's calls now succeeding instead
of reading a placeholder.

**The oracle-free crunch has reached its honest edge.** Tonight, across ~ten loop ticks, took PPSA04263
from a 32-call death to a third of a million calls deep and left every title with fewer placeholder
stubs. The fixes that moved walls:

- D460 map commits into a reservation (blast radius: 3 titles)
- D463 the mapping arena moved off the thunk data-block base (a real address collision)
- D464 per-thread thread-local storage for spawned threads (10k -> 333k calls on PPSA04263)
- D465 blocking WaitEventFlag (killed a 304k-call spin)

plus the return-value (D459) and reserve-failure (D462) diagnostics that made two of those findable, and
a run of standard-library stubs (putchar, deterministic random_device D461, strcpy_s, atan2f, sincosf,
four pthread attr setters, and these two).

What is left needs data this loop cannot produce: the two `int 0x41` guest asserts (PPSA04263 0x196b91a,
PPSA21564 0x11ccd) want the assert's read traced back to the wrong value - deep RE or an obSCEne trace;
PPSA25872's spin wants the unnamed `PS5Util::0xf948d02a4f9f5ace` named (model vocabulary or hardware);
`_Getpctype` wants the ctype table off a console; PPSA28061 wants the GPU. Next tick runs a full corpus
sweep to measure the night's cumulative progress and confirm whether any freshly-exposed wall is still
oracle-free; if not, the loop is honestly done until there is data or a person.
