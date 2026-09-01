# D251 - The console's filesystem is a knowledge file, and a title's data layers over it


**decided** · 2026-08-25 · designed before there was more than one path in it

Two designs were on the table: a 1:1 tree of the console's directories, for accuracy, and a
per-title overlay, for keeping one title's data in one place.

**They answer different questions and both are right.** The guest-visible namespace is the
only part a guest can observe - it resolves paths and opens them, and every answer either
matches the console or does not - so it should be 1:1. Where the host actually keeps the
bytes is unobservable, so it should be whatever is cleanest, which is per-title. The mount
table is already the indirection between the two, and making a mount a *stack* rather than a
directory buys the merge for a list walk per resolve.

Layered in process, never on disk. Merging on disk would mean a copy of the base tree per
title and no way to tell afterwards which file came from where; layering keeps the base
derived and rebuildable, which is the test that it really is derived.

### The base is a manifest, not a committed tree

"What we know a real PS5 filesystem looks like" is a set of claims about the platform, and a
claim here says how it is known. A committed directory tree cannot carry that, and git does
not track empty directories, so it would have to be faked with placeholder files to survive
a clone.

`crates/orbistoun-fs/data/filesystem.toml` is therefore shaped like the function knowledge
base: a path, whether a guest may write there, `known_by`, and what asked for it.

**It has two entries.** `/app0`, because six titles read from it. `/data`, because the probe
asked an hour ago. Nothing else, and that is the point: a guest resolving a path orbistoun
invented gets a fabricated platform fact back, which is worse than the failure it replaces -
the failure is information. The tree grows when something asks, and the probe is the
instrument for asking (D242 applies to directory names as much as to symbol names).

Writability comes from the manifest rather than from a constant in code, so the two cannot
drift.

