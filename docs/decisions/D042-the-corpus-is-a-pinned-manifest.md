# D042 - The corpus is a pinned manifest

**Status:** decided
**Date:** 2026-09-26

`corpus/sources.toml` lists third-party homebrew by source, licence and per-asset hash, and
`orbistoun-corpus` fetches the bytes into the titles library. The manifest is tracked; the
bytes never are. CI does not fetch on every run.

**Why:** a real guest getting further is the measure of progress, and it needs guests nobody
here wrote. Pinning by hash keeps the corpus reproducible past the day a branch moves.
Downloading is not redistributing, and a red build caused by someone else's repository moving
teaches people to ignore red builds.

**Rejected:**
- Tracking the binaries: somebody else's guest bytes in the tree.
- Pinning by branch: not reproducible within a month.
- Clone-and-compile sources: every developer needs a cross-compiler.
