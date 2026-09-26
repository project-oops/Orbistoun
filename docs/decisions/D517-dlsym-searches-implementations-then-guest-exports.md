# D517 - `dlsym` searches implementations, then guest exports

**Status:** decided
**Date:** 2026-09-03

`sceKernelDlsym` answers from orbistoun's implemented-function table first and from the guest's
own module exports second. The loader registers each export as a hash and address, and the
kernel hashes the requested name at the call, so `orbistoun-kernel` depends on `orbistoun-nid`.

**Why:** a guest asks for a name and every export table is keyed by a hash of one; the loader
cannot precompute which names will be asked for, and a hash cannot be reversed. Adding guest
exports after the implementation table is purely additive: every name that resolved before
resolves to the same address.

**Rejected:**
- Resolving against the implementation table alone: a guest is told its own exported symbol does not exist.
- Guest exports first: changes what already-working names answer, on no evidence.
- A precomputed name map in the loader: needs the names before any guest has asked.
