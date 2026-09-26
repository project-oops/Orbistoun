# D113 - Translations cached by content

**Status:** assumed
**Date:** 2026-09-26

The translation cache is keyed by a hash of the shader's decoded extent: decode, hash, look up,
translate. A hit re-checks the length and both end words, and a mismatch is refused. The cache
is stored per title in the title library, and every shader the title's corpus records is
translated into it before the run, so a draw finds its translation already made.

**Why:** a guest moves shaders, reuses addresses and holds one shader at two addresses. An
address-keyed cache serves the old translation for a new shader and nothing indicates it. A
content key is also stable across runs, which is what lets translation happen ahead of the
first draw.

**Rejected:**
- Keying by address: stale hits drawn with code the guest has replaced.
- Keying by the read window: identical shaders followed by different data never hit.
