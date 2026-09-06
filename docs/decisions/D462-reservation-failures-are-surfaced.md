# D462 - A reservation the guest could not make is named in the run report


**measured** - 2026-09-01 (user-directed /loop: overnight oracle-free crunch)

A failed reservation was invisible twice over. `map_named_direct_memory` collapses every
[`MemError`] into `NoMemory`; the guest reads that as out-of-memory and faults through the null
its allocator kept, far from the reservation, with the base and the reason both lost. And on
**Windows the failure was mislabelled**: `platform::reserve` reported every `VirtualAlloc` null
as `Conflict` regardless of `GetLastError`, the exact D010 gap the Unix path had already closed
(EEXIST/ENOMEM vs the rest). A whole afternoon of worklog 284 went into ruling out host commit,
the base in isolation, the region list and the direct pool by hand, precisely because the run
would not say which of them it was.

Two changes make it legible:

- **Windows `reserve` distinguishes the cause.** `ERROR_INVALID_ADDRESS` is a real conflict (the
  base is already reserved); anything else is the host refusing (a commitment limit, not-enough-
  memory), reported as `HostRefused` with the code. The D010 rule, finally met on both platforms.
- **`AddressSpace::reserve` and `protect` record their last failure** - base, length, and a reason
  in words - into allocation-free statics (`orbistoun_mem::last_reserve_failure`), because a guest
  map runs `reserve` on the guest's own stack and a lock or allocation there is the D381 fault. The
  worker's `persist` names it once, from the host side, where a print is safe.

**Verified by what it then found in one run.** PPSA04263's wall reported
`base=0x720000000000 len=0x32000000 - conflict`, and `0x720000000000` is
`orbistoun_thunk::SUGGESTED_DATA_BASE` exactly - the address-space collision of D463, which the
hand investigation had failed to see across a whole tick and this named immediately. The base was
the missing fact: the empty-arena reasoning could not confirm it, and it was the whole answer.
