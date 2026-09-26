# D414 - The test corpus is a manifest-driven fetch tool

**Status:** assumed
**Date:** 2026-08-31

The test corpus is fetched from a tracked manifest naming each source and how
to obtain it - a tagged release with a pinned, verified checksum, or a path
into a sibling checkout - into an untracked, per-source directory; the fetch
logic lives in its own crate and the command line is a thin front end over it.

**Why:** Tracking only the manifest keeps guest bytes out of the repository
while keeping exactly how to reproduce the corpus in version control. Pinning
and verifying a checksum for a tagged release catches a silent change under a
tag that is supposed to be fixed; a sibling checkout with no published release
yet is re-hashed each time instead, since there is no fixed point to verify
against.

**Rejected:**
- Tracking guest bytes directly: the provenance boundary this project keeps
  excludes committing guest material.
- Fetching from a branch rather than a tag: a branch can move under a pinned
  reference without anyone noticing.
