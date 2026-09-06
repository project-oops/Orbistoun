# 2026-09-02 - (/loop) The differential calls back into guest code, and `qsort` agrees

```
differential cases    63  ->  76   (76 rebuilt, 76 agreed, 0 diverging)
tests               1947  -> 1950
```

Thirteen cases across `qsort` and `bsearch` - the first where the reference and orbistoun have
to agree about **calling back into the caller's own code**, not merely about a return value.

## Why these were the interesting ones left

Every case so far handed orbistoun data and compared what came back. A comparison-sorted array
is different: orbistoun has to invoke a function the caller supplied, in the guest's calling
convention, once per comparison, and get the sense of the answer right.

It turns out to be straightforward for the reason the whole project rests on: **guest code runs
natively in this process.** `GuestComparator` is `extern "sysv64" fn(u64, u64) -> u64` reached
by transmuting an address, so a test writes exactly that function and hands over its address.
There is no bridge to build - the emulator calls it as it would call a guest's.

Cases: jumbled, already sorted, reversed, duplicates, a single element, negatives; and for
`bsearch` the first, middle and last elements, three kinds of absent, and an empty array.
**All thirteen agree.**

**`qsort/jumbled` passing is the proof the callback ran at all.** A `qsort` that did nothing
would still match `already-sorted`, which is exactly the sort of false pass a differential has
to be built to avoid - so the jumbled case is doing double duty as the liveness check.

## Two things the record format needed

**A byte blob is not text.** A four-byte `5` is `05 00 00 00`, so element data cannot ride in a
NUL-terminated field at all. `b:` carries plain hex, and an odd digit count is **refused**
rather than rounded - half a byte means the record was truncated, and a case built from a
truncated array would be compared against the wrong input while looking like it worked.

**A comparator is code, and that is the one place the "one case list" property does not reach.**
The reference cannot hand its machine code to the checker, so the case names a *semantic* -
`int32-asc` - and each side implements it. A name the checker does not know answers `None`,
which the rebuild gate reports rather than skipping: an unrecognised comparator must not become
a silently absent case. That is stated in both files, because it is the seam where the two
sides could drift and nothing else would notice.

## State

`cargo test --workspace` green - **117 suites, 1950 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. `also differential` reports `76 cases, matching
tools/differential/reference.c`.

The agreement count was re-checked by deliberately breaking it: 76 rebuilt, 76 agreed, 0
diverging.

Nothing committed. The day holds worklogs 292-322 and D466-D479.

**Next**: `strtok`, whose answer depends on state carried between calls - a single case cannot
express it, so the record format needs a sequence, and that wants designing before it is
written. Then the sign-extended return width, which wants a decision before a sweep: nine
measurements say the console extends and orbistoun does not, and whether that is per function
or the whole vendor family is the question to settle first.
