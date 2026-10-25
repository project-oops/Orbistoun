# D960 - A computed image descriptor is found where it is read

**Status:** decided
**Date:** 2026-10-25
**known_by:** assumed

A dispatch's image access whose descriptor registers were rewritten since an earlier access, by
something no traced table load or user-data word names - an arithmetic rewrite, a load out of a
buffer the user data names, paths that meet with different loads - is a texture of its own, keyed by
the access: its byte offset and the descriptor's first register. Later accesses through the same
registers with nothing written between share it. The host finds the descriptor for each dispatch by
running the program before the access over the dispatch's user data and guest memory and reading the
eight registers there, as a computed buffer's is found (D753). The group's ids are left unknown, so
a descriptor that differs from group to group is refused rather than bound for one group. Pixel and
primitive shaders still refuse (D690).

**Why:** register identity (D690) is safe but refuses any program that reuses registers for a second
texture. Grand Theft Auto V's full-screen compute pass at `0x74005b809c00`, 240 by 270 groups, reads
two textures through `s[0:7]`, and its scene went unrendered. Running the program before the access
is exact wherever the descriptor depends only on user data and memory, which is what a full-screen
pass's textures do.

**Rejected:**
- Keying by register with a rewrite count: names a second texture but not where its descriptor comes
  from.
- Running every wave for an image: a texture binding is one image for the whole dispatch, so a
  descriptor that varies by wave cannot be bound however it is found.
- Extending the static tracer to each new rewrite form: one rule per program shape, and untraced
  forms keep arriving.

**Amended by D983:** a descriptor whose registers hold a record the program selected from a table at
run time is not run for. Every image the table names is bound in an array, and the module finds the
one the registers hold as it runs, so a descriptor that varies by wave or by group is read exactly.
