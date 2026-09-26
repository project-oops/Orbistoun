# D151 - Handles follow what the guest does with them

**Status:** decided
**Date:** 2026-09-26

A handle the guest dereferences - thread, lock, file - is the address of a leaked, zeroed block
that is never written or freed. A handle the guest only compares and passes back - a video port,
a semaphore - is a small integer from one. Every guest-supplied handle is checked as issued
before use.

**Why:** the guest reads fields through some handles, and a small integer there faults at a
low address. The real layouts are known from no lawful source, so zero is the honest content: a
pointer field reads null and the guest takes its own error path. An integer handle starts at one
because callers test against zero.

**Rejected:**
- Opaque integers for every handle: faults where the guest dereferences.
- Plausible field values in the block: invented behaviour.
- Addresses for every handle: truncation collides where the ABI slot is four bytes.
