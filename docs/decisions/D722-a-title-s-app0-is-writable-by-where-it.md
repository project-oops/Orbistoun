# D722 - A title's /app0 is writable by where it is staged, not what it ships

**Status:** decided
**Date:** 2026-09-26

A title's storage origin decides whether its `/app0` is writable: a library image keeps a
read-only `/app0` (D250), and a title staged under `/data/homebrew/<id>` - a module in the
library's staging tree, or a run with `--staged` - has `/app0` and `/data/homebrew/<id>` as one
directory under one writable overlay layer, so a file written through either path is seen through
the other.

**Why:** on the console permissions come from the mount, not the package: a staged title runs with
`/app0` as its directory on the read-write user partition, and a port such as Neverball writes its
user data there (oops-apps `neverball/shim/nb_start.c`). Writes land in the per-title overlay
(D422) and never in the library's files: a file that exists only in a lower layer is copied up
first, and removing or renaming a name a lower layer also holds is refused, because a correct
answer needs a whiteout this overlay does not keep.

**Rejected:** deciding by package metadata (a `param.json` field, a content-ID prefix) - the
console does not, and a title would choose its own permissions.
**Rejected:** a writable `/app0` for every title - a retail image's `/app0` refuses writes.
**Rejected:** writing through to the first layer that holds a file - it writes the library.
