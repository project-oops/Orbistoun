# Roadmap

Committed next steps, in dependency order. Anything not here is in
[BACKLOG.md](BACKLOG.md); rejected directions are in [SCOPE.md](SCOPE.md). The
reasoning behind each shape is in [DECISIONS.md](DECISIONS.md).

The ordering rule: **each phase must produce an observable result on its own.** The
alternative - building the container parser, the address space, and the GPU layer
before anything can be run - is how a project reaches month six with nothing to show
and no way to tell which of three layers is wrong.

Phases 1-6 are a strict chain, with 1b and 2b hanging off 1. Phases 0b through 0e are
independent of everything and of each other.
**This table is generated.** Edit an item under `roadmap/`, then run
`tools/split-doc.sh --index orbistoun ROADMAP 2 roadmap`.

| | item | status |
|---|---|---|
| ⚪ | [Where this actually stands (2026-08-24)](roadmap/001-where-this-actually-stands-2026-08-24.md) | no marker |
| ⚪ | [Phase 0 - Synthetic fixtures](roadmap/002-phase-0-synthetic-fixtures-reduced.md) | no marker |
| 🟢 | [Phase 0b - ABI spike](roadmap/003-phase-0b-abi-spike-done-both-platforms.md) | done |
| 🟢 | [Phase 0c - Structural seams](roadmap/004-phase-0c-structural-seams-done.md) | done |
| 🔴 | [Phase 0d - Test corpus tooling](roadmap/005-phase-0d-test-corpus-tooling-not-done.md) | not done |
| 🟢 | [Phase 0e - Observability substrate](roadmap/006-phase-0e-observability-substrate-done.md) | done |
| 🟢 | [Phase 1 - Container wrapper and dynamic segment](roadmap/007-phase-1-container-wrapper-and-dynamic.md) | done |
| 🟢 | [Phase 1b - Corpus-wide survey report](roadmap/008-phase-1b-corpus-wide-survey-report-done.md) | done |
| 🟢 | [Phase 2 - Symbol resolution](roadmap/009-phase-2-symbol-resolution-done.md) | done |
| 🟢 | [Phase 2b - GUI shell and library](roadmap/010-phase-2b-gui-shell-and-library-done-no.md) | done |
| 🟢 | [Phase 3 - Address space](roadmap/011-phase-3-address-space-done-both.md) | done |
| 🟢 | [Phase 4 - Worker, placement, relocation, protection, stubs, entry; thread pointer, trace sink](roadmap/012-phase-4-worker-placement-relocation.md) | done |
| 🟡 | [Phase 5 - Threading and synchronisation](roadmap/013-phase-5-threading-and-synchronisation.md) | begun |
| ⚪ | [Phase 6 - First pixel](roadmap/014-phase-6-first-pixel-contents-being.md) | no marker |
| ⚪ | [Phase 6's contents, built ahead of it](roadmap/015-phase-6-s-contents-built-ahead-of-it.md) | no marker |
| ⚪ | [Running alongside](roadmap/016-running-alongside.md) | no marker |
| ⚪ | [Stretch](roadmap/017-stretch.md) | no marker |
| ⚪ | [Not on the roadmap](roadmap/018-not-on-the-roadmap.md) | no marker |
| ⚪ | [Answering Prosperous](roadmap/019-answering-prosperous.md) | no marker |

| | meaning |
|---|---|
| 🟢 | done |
| 🟡 | begun |
| 🔴 | open, or explicitly not done |
| ⚪ | deferred, not planned, or carrying no marker either way |
