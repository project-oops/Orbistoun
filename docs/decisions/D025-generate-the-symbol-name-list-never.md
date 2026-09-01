# D025 - Generate the symbol name list; never ship a database

**decided** · 2026-08-19

NIDs are hashes of names, so a name list plus a matcher beats obtaining someone's
database. The naming convention is highly regular (`sce<Module><Verb>`, with the
kernel surface largely POSIX renamed), so a candidate generator hashed against the
NIDs a real module actually imports gives **proof by collision** - self-verifying,
and requiring no vendor binary be read.

Third-party databases may be loaded at runtime, but the thing to check is not the
names (interface identifiers are facts) - it is **how the database was produced**.
Brute-forced from names is clean; dumped from decrypted firmware carries the
provenance problem into our tree. Either way, none is distributed with this repo.

Implies a name-candidate generator in `orbistoun-nid`, which needs real NIDs to
match against and is therefore gated on having a real module to read imports from.

