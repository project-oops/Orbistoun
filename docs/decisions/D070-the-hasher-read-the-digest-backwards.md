# D070 - The hasher read the digest backwards, from the very first commit

**decided** · 2026-08-19

`NidHasher` took the first eight digest bytes **little-endian**. It should be
**big-endian**. Every NID this project has ever computed was wrong.

**Why nothing caught it.** Every test hashed a name with an arbitrary suffix and
compared the result against its own output. That is self-consistent, and self-consistent
with a wrong byte order. The decoder was right and the hasher was wrong, and the two
never met - `decode_symbol_name`'s own documentation admitted as much, saying the choice
was "consistent but not independently verified".

**What exposed it.** Hashing published C library names against a real import table. The
right convention matched 66 of 468; every other combination of byte order and suffix
placement matched zero. There is no ambiguity in a signal like that.

**The invariant that was missing**, now a test: a hash must survive a round trip
through the symbol-name encoding. `decode(encode(hash(name))) == hash(name)`. That
needs no suffix, no real module, and no external reference - it compares the two halves
of this crate against each other, which is exactly what nothing was doing. `encode_nid`
exists mainly so that test can.

**The general lesson**, worth more than the fix: a test that compares a component to
itself proves only internal consistency. Both halves of a two-sided convention can be
individually well-tested and jointly wrong. Where two representations must agree, test
the *agreement*.

