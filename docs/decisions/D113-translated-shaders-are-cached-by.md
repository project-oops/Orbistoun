# D113 - Translated shaders are cached by content, never by address


**Status:** decided (confirmed with input, 2026-08-20)

The cache key is a hash of the shader's bytes.

An address-keyed cache is wrong in three ways and right in the common one, which is the
worst combination available. A guest may move a shader (a miss that should have been a
hit), write a different shader to the same address (a **hit that should have been a
miss** - the frame is then drawn with code that is no longer there, and nothing indicates
it), or hold the same shader at two addresses (a needless second translation).

**The order of operations is the substance.** Decode, then hash, then check the cache,
then translate. Decoding first is what makes the key the shader's actual extent rather
than the arbitrary window it was read from - otherwise two identical shaders followed by
different data miss every time. Decoding is a linear walk and translation expands every
instruction across sixty-four lanes, so the expensive half is the one behind the cache.

### Confirmed, and a hit now verifies

Content keying stands - a guest reuses addresses, so an address-keyed cache serves the old
translation for a new shader, and nothing else fixes that.

**The hash width was never the weak part.** Sixty-four bits over a few thousand shaders is
a collision probability around one in a million million. The real exposure was that a hit
was served *without ever looking at the bytes again*, so a key that stopped meaning what it
did would serve a stale module silently. That is not hypothetical: the key is computed over
the decoder's idea of where a shader ends, and that number changed once already when the
decoder stopped halting at the first end-of-program instruction.

A hit now checks the length and both end words, and a mismatch is refused loudly rather
than re-translated - both possible causes are faults in this crate, and recovering quietly
would leave the cache in a state nobody can reason about.

**The check found a bug in itself.** Reading a word past the end answered zero, so any two
different shaders shorter than a word compared equal - the exact fault the type exists to
prevent, at the one end nobody thinks about. Short reads are zero-padded now.

