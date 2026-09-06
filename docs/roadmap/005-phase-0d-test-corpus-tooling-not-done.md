# Phase 0d - Test corpus tooling *(done)*


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

## Done, and the marker was stale rather than the work missing

Checked against the four things this item actually asked for, on 2026-09-03:

- **`orbistoun-cli corpus`** exists with `list`, `sync` and `run`. `run` is more than was
  asked - it turns the loop over every guest and records to `compat/`.
- **`corpus/sources.toml`** names two sources and **26 hash-pinned assets**, prebuilt
  release assets rather than clone-and-compile, exactly as the item preferred.
- **A licence field per entry**, both present.
- **`/titles/*` is gitignored.**

The second half - *toolchain setup plus one minimal test app built with the open toolchain* -
is the second source: obSCEne, which is our own source built with an open toolchain and which
orbistoun runs through twenty-seven conformance sections. A real container with clean
provenance, which is the whole reason this phase was pulled early.

**Nothing was built to close this.** The item had been finished and its marker never moved,
which made the roadmap report one red phase that was not red. Fourth stale work item found this
week, after `/dev/random`, the differential's "missing" cases, and D488's note that the
oscillation does not move the verdict.

