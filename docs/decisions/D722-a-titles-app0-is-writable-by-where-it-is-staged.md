# D722 - A title's /app0 is writable by where it is staged

**Status:** decided
**Date:** 2026-09-26

A title's storage origin decides whether its `/app0` is writable: a library image keeps a
read-only `/app0`, and a title staged under `/data/homebrew/<id>`, as a module in the library's
staging tree or a run with `--staged`, has `/app0` and `/data/homebrew/<id>` as one directory
under one writable overlay layer, so a file written through either path is seen through the
other.

**Why:** on the hardware permissions come from the mount, not the package: a staged title runs
with `/app0` as its directory on the read-write user partition, and ports write their user data
there. Writes land in the per-title overlay and never in the library's files; a file only in a
lower layer is copied up first, and removing or renaming a name a lower layer also holds is
refused, because a correct answer needs a whiteout the overlay does not keep.

**Rejected:**
- Deciding by package metadata such as a `param.json` field or a content-ID prefix: the hardware
  does not, and a title would choose its own permissions.
- A writable `/app0` for every title: a retail image's `/app0` refuses writes.
- Writing through to the first layer that holds a file: it writes the library.
