# D639 - Twenty blank pages nobody mentioned

**Status:** measured
**Date:** 2026-09-09

## A fault with three null registers and no explanation

PPSA21564 runs half a million calls at **100% standing** - two calls on stubs out of 500,260 - and
dies in its own module's static initialisation:

```text
! the guest faulted at the title's own modules+0x7af792, read of 0x38
    read of 0x38 is a null pointer plus an offset
    rax=0x0 rcx=0x0 rdi=0x0 … r12=0x720000007000 r13=0x720000007000
    just before: libc::__cxa_guard_release(0x40000e2d2788) -> 0x0
    just before: libc::__cxa_guard_acquire(0x40000e2d2788) -> 0x1
```

`__cxa_guard_acquire` answering **1** means *run the initialiser*, and the tail before it is
hundreds of `libc::strcmp` calls from one site, all against the same string, all non-zero: a linear
search by name that never matches, then a null.

`r12` and `r13` printed as bare numbers. `0x7200_0000_0000` is `SUGGESTED_DATA_BASE` - storage for
imports that name **data** rather than functions (D323) - and nothing in the report said so.

## Twenty of them, and most are C++ vtables

```text
libc  _ZTVN10__cxxabiv120__si_class_type_infoE  [data]
libc  _ZTVN10__cxxabiv117__class_type_infoE     [data]
libc  _ZTVSt9bad_alloc                          [data]
libc  _ZSt21_sceLibcClassicLocale               [data]
libc  _Stdout / _Stderr, libkernel __stack_chk_guard, …    20 in all
```

Each is served as **one zeroed page** (D307). A zeroed vtable is a table of null function
pointers, so the first virtual call through one reads null and dies at a small offset - which is
what this title does, at `+0x38`.

Whether that is *this* fault's cause is not asserted here. What is asserted is that the run handed
the guest twenty blank pages and said nothing about any of them, so nobody reading the fault could
connect the two.

## Three omissions, one after another

Naming them took three fixes, and each was invisible behind the one before it:

1. **The region is not one of the five** the fault handler knows (image, stubs, stack, guest
   mappings, title modules), so `locate` returned nothing and the address printed bare.
2. **The reverse lookup did not exist.** `DATA_SYMBOLS` has been published as name-to-address
   since D344; nothing went the other way. `data_symbol_at` does, by **exact page** - the blocks
   are contiguous, so a nearest-preceding search would name the block before a gap as though it
   owned the address.
3. **The pages were never published as readable.** The register annotator only emits a line for an
   address whose bytes it can read, so with the first two fixed it still printed nothing. Exactly
   the omission D593 found for the title's own modules, in the region beside them.

```text
r12 -> data f7uOxY9mM1U#r#n+0x0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

Sixteen zero bytes, named as a data import.

## What it still cannot say

The key is the **raw import string** - the encoded NID - not the decoded symbol. That is
deliberate elsewhere and load-bearing: `data_symbol("optarg")` is how an implementation finds the
guest global it owns (D344), so re-keying the map would break the lookup it exists for. A separate
display name is the fix and it is not done.

`data f7uOxY9mM1U#r#n` is worse than `data _ZTVSt9bad_alloc` and enormously better than
`0x720000007000`, which is what it said this morning.
