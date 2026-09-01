# Where this actually stands (2026-08-24)


| Phase | State |
|---|---|
| **0** - synthetic fixtures | **not done**, and no longer blocking. Real material arrived first (D050), so this narrowed to the malformed cases a compiler never emits |
| **0b** - ABI spike | **done**, both platforms. Grew into `orbistoun-thunk` |
| **0c** - structural seams | **done**. `cargo tree` shows `orbistoun-gpu` with no path to `ash`, and the shims hold no logic |
| **0d** - test-corpus tooling | **not done**. There is no `corpus sync`; the corpus is material dropped in by hand |
| **0e** - observability substrate | **done**. Run reports, retention, and the diff-against-previous machinery |
| **1** - container wrapper and dynamic segment | **done** |
| **1b** - corpus-wide survey report | **done**. `./bin/orbistoun sweep` and `orbistoun-cli worklist` |
| **2** - symbol resolution | **done**. Both halves: a database loads from disk, and the generator confirms names by collision |
| **2b** - GUI shell and library | **done**; no output surface |
| **3** - address space | **done**, both platforms verified |
| **4** - worker, placement, relocation, protection, stubs, entry | **done** |
| **5** - threading and synchronisation | **begun, and nowhere near its own observable result** |
| **6** - first pixel | **not started**; its *contents* are being built ahead of it |

**The honest reading of that table.** Phase 5 is listed as current and is not what the
work is actually about: mutexes and semaphores are built and exercised, but
`scePthreadCreate` has never been called by any title, so every guest is still in
single-threaded startup. The three walls in [PROJECT_STATUS.md](../PROJECT_STATUS.md) are
**phase 4 completion problems** - getting one guest through startup - and until one of
them falls, phase 5 has nothing to demonstrate against.

