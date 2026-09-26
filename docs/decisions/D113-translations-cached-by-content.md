# D113 - Translations cached by content

**Status:** decided
**Date:** 2026-08-20

The translation cache is keyed by a hash of the shader's decoded extent: decode, hash, look up,
translate. A hit re-checks the length and both end words, and a mismatch is refused.

**Why:** a guest moves shaders, reuses addresses and holds one shader at two addresses. An
address-keyed cache serves the old translation for a new shader and nothing indicates it.

**Rejected:**
- Keying by address: stale hits drawn with code the guest has replaced.
- Keying by the read window: identical shaders followed by different data never hit.
