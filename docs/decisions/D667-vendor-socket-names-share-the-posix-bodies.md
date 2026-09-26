# D667 - Vendor socket names share the POSIX bodies

**Status:** decided
**Date:** 2026-09-10

The socket bodies live in `orbistoun-fs`, which owns the descriptor table, and return a value or
the POSIX errno that stopped them. `orbistoun-fs` encodes that as `-1` plus `errno`;
`orbistoun-net`, which declares `libSceNet`, offers the `sceNet*` names and encodes the same answer
as `0x8041_0100 | errno`. A failure nothing can name is errno zero.

**Why:** the crate that declares a library offers its names wherever the body lives. Files and
sockets share one descriptor numbering, so the bodies cannot split. The two spellings agree on
success and differ on failure, so each crate encodes the shared answer its own way. Errno zero
never collides with a measured code and invents no explanation.

**Rejected:**
- Aliasing vendor names onto the POSIX functions: the failure encodings differ.
- A separate socket table in `orbistoun-net`: gives a guest two descriptor numbering spaces.
- A plausible errno such as `EINVAL` for an unnamed failure: invents a constant.
