# D113 - Translations cached by content

**Status:** assumed
**Date:** 2026-09-26

The translation cache is keyed by a hash of the shader's decoded extent: decode, hash, look up,
translate. A hit re-checks the length and both end words, and a mismatch is refused. The cache
is stored per title in the title library. Each entry records the shader's bytes and what it was
translated against beside the module, so a later run serves the module without translating,
and before the guest starts every entry another translator made is translated again from its
recorded shader. A draw finds its translation already made. The translator is named by the build
and a digest of its sources and tables taken at build time, so a build from an edited tree is
another translator though it names the same commit.

**Why:** a guest moves shaders, reuses addresses and holds one shader at two addresses. An
address-keyed cache serves the old translation for a new shader and nothing indicates it. A
content key is also stable across runs, which is what lets translation happen ahead of the
first draw.

**Rejected:**
- Keying by address: stale hits drawn with code the guest has replaced.
- Keying by the read window: identical shaders followed by different data never hit.
