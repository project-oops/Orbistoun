# 464. Twenty blank pages

**2026-09-09** - directed, continuing 463

Bus idle a thirteenth pass. Kept reading ordinary runs of guests nobody had investigated - the
third pass running that this has been where the finding was.

## PPSA21564 dies in static initialisation, and the report could not say why

Half a million calls at **100% standing** - two on stubs out of 500,260 - then:

```text
! the guest faulted at the title's own modules+0x7af792, read of 0x38
    rax=0x0 rcx=0x0 rdi=0x0 … r12=0x720000007000 r13=0x720000007000
    just before: libc::__cxa_guard_acquire(0x40000e2d2788) -> 0x1
```

`__cxa_guard_acquire` answering 1 means *run the initialiser*, and before it are hundreds of
`strcmp` calls from one site, all against the same string, all non-zero - a search by name that
never matches, then a null.

`r12` and `r13` printed as bare numbers. `0x7200_0000_0000` is the storage for imports that name
**data** rather than functions, this title has **twenty** of them, and most are C++ vtables -
`_ZTVSt9bad_alloc`, `_ZTVN10__cxxabiv117__class_type_infoE`, `_ZSt21_sceLibcClassicLocale`. Each is
served as one **zeroed page** (D307), and a zeroed vtable is a table of null function pointers.

The report said nothing about any of them (D639).

## Three omissions stacked behind each other

Each was invisible until the one in front was fixed:

1. The data region is **not one of the five** the fault handler knows, so `locate` returned nothing.
2. **No reverse lookup existed.** `DATA_SYMBOLS` has gone name-to-address since D344 and nothing
   went the other way. `data_symbol_at` does now, by exact page - the blocks are contiguous, so
   nearest-preceding would name the block before a gap as though it owned the address.
3. **The pages were never published readable**, so the register annotator - which only emits a line
   for bytes it can read - still printed nothing with the first two fixed. The same omission D593
   found for the title's own modules, in the region beside them.

```text
r12 -> data f7uOxY9mM1U#r#n+0x0 = 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
```

## Surprises

- **Two calls on stubs out of half a million.** This title is almost entirely served, and still
  dies - the wall is not a missing function, it is the *contents* of what is served.
- **The key is the encoded NID, not the symbol.** That is load-bearing: `data_symbol("optarg")` is
  how an implementation finds the guest global it owns (D344), so re-keying breaks the lookup the
  map exists for. A separate display name is the fix; not done, and recorded as not done.

## Next

- Whether a zeroed vtable is *this* fault's cause. The report can now be read for it; nothing here
  asserts it.
- The blocked list from 461, unchanged.
