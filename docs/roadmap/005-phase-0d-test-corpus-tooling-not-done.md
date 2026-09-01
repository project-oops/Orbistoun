# Phase 0d - Test corpus tooling *(not done)*


Two pieces, both from D042 and D044:

- **`orbistoun-corpus`** - a manifest of homebrew pinned by commit or release-asset
  hash, downloaded into a gitignored `titles/`, with a licence field per entry.
  Prefers prebuilt assets over clone-and-compile so a cross-compiler is not a
  prerequisite for every developer.
- **Toolchain setup plus one minimal test app**, built with the open toolchain.

**Why early, and why it partly changes phase 0.** A test app we compile ourselves is
a **real container with clean provenance** - we wrote the source, so we know exactly
what is in it. That is better input for the parser than a byte-crafted fixture for
the happy path. Synthetic fixtures remain necessary for the malformed cases a
compiler will never emit.

**Observable result:** `orbistoun-cli corpus sync` populates `titles/` reproducibly
on a clean machine, and one genuine container exists to point the parser at.

