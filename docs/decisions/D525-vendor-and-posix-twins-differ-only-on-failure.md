# D525 - Vendor and POSIX twins differ only on failure

**Status:** decided
**Date:** 2026-09-03

A vendor-named call with a POSIX twin (`sceKernelStat` and `stat`, `sceKernelPread` and `pread`)
shares the twin's success path and written structures, and answers failure in the vendor encoding
`0x8002_00xx` where the POSIX form answers `-1`. The vendor name is registered under the library
that declares it.

**Why:** the two agree on success and disagree on failure, and a caller tests a vendor call for a
negative 32-bit code that `-1` is not. Sharing the success path keeps one definition of each
structure. Which errno the console answers for a given failure is often unmeasured, so tests pin
the family and sign a caller branches on.

**Rejected:**
- Registering the POSIX function under the vendor name: answers `-1` where the caller tests a vendor code.
- A separate implementation per name: two definitions of one structure that drift apart.
