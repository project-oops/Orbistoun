# 2026-09-02 - (/loop) Bulk port batch 2: the runtime's out-of-line atomics, conversions and assert

Second batch under D472, and the first where the honest answer to part of the list was **"not
yet, and here is why"**.

```
documented   711 needed, 441 missing   ->   711 needed, 434 missing
```

## What went in - seven functions

**The C11 atomics** (new `orbistoun-libc/src/atomic.rs`): `_Atomic_load_4`, `_Atomic_fetch_add_4`,
`_Atomic_fetch_sub_4`, `_Atomic_compare_exchange_weak_4`. `<stdatomic.h>` is mostly compiler
intrinsics, but a runtime that must work without them ships out-of-line copies and calls those;
the Dinkumware runtime this platform carries (D468) names them this way.

Two decisions worth stating:

- **The memory-order argument is deliberately ignored, upward.** Each takes one, and the
  strongest order is a conforming implementation of every weaker one - `SeqCst` never permits a
  reordering `Relaxed` forbids - so all four use `SeqCst` rather than mapping an enumeration
  whose numbering is the runtime's own and is not established here. A deliberate strengthening
  costs a little speed and cannot give a wrong answer; guessing the enumeration could.
- **A misaligned object is refused rather than dereferenced.** C11 requires atomic objects to be
  aligned (6.2.8), so a misaligned address is a guest already in undefined territory - but
  forming a misaligned reference would make it undefined *here* too, and unlike the guest's
  mistake ours would be silent.

**Conversions**: `_Stoul`, `_Stoull` - the runtime's own names for what `strtoul`/`strtoull`
wrap. Delegated to the existing implementations rather than reimplemented: two copies of one
parser is two answers to one question, and the copy nobody exercises is the one that drifts.

**`_Assert`** - the runtime routes a failed `assert` here instead of calling `abort` itself, so
this is where a guest says which assertion it lost. Message to stderr *and* the kernel log
first, then the ordinary abort stop; stopping without the message would throw away the only part
a reader needs (ISO/IEC 9899 7.2.1.1).

Six tests, each guard made to fail with its passing case beside it. The two that matter:
`fetch_add` answering the **previous** value (answering the sum passes every single-threaded
eyeball test and breaks every ticket lock), and a failed compare-exchange **writing the actual
value back through `expected`** - without which the caller's retry loop never terminates.

## What was deliberately left out, and why

- **`_FCosh`, `_FSinh`, `_FExp`, `_FDtest`, `_FNan`.** These are not the C library's `coshf` and
  friends: they are the runtime's internal helpers with their own argument conventions, and
  `_FDtest` answers a classification using **the runtime's own constants**, which are not
  established here. Writing them would mean inventing those numbers, which is the one thing
  principle 3 forbids outright. `_FNan` is a data object rather than a function besides.
- **`_Lockfilelock`, `_Unlockfilelock`, `_Locksyslock`, `_Unlocksyslock`.** Implementable, but
  correctly: they are recursive locks, and the tempting version - a no-op, since much of a guest
  is single-threaded early on - is a silent correctness hazard the moment two threads share a
  `FILE`. They need a real recursive lock keyed by the argument, which is a batch of its own
  rather than four one-line stubs.
- **`_Thrd_id`, `_Thrd_join`.** These belong beside the `_Mtx_*`/`_Cnd_*` family in
  `orbistoun-kernel`, not in libc, and want the thread registry rather than a shim here.

## `_Stdout` / `_Stderr` - not a new design question after all

Checked before writing, as the plan said to. A data-import mechanism **already exists** (D323):
`orbistoun-thunk` reserves storage per named data import and publishes the addresses, and
`data_symbol(name)` reads one. So these need no new concept - they need the right *contents*: a
`FILE *` the rest of libc recognises. `fprintf` already routes by wrapped descriptor
(`orbistoun_fs::open::wrapped_descriptor`) and falls back to the host's error stream otherwise,
and `wrap_descriptor(fd)` mints exactly the handle required. So the work is to wrap descriptors
1 and 2 at start-up and store those handles in the two objects - real work, but plumbing rather
than a decision. No D473 needed.

clippy `--tests` clean, fmt clean, orbistoun-libc tests pass, identity scan clean, nothing
committed.

**Next**: `_Stdout`/`_Stderr` by the route above, the four recursive locks done properly, then
batch 3 - the Itanium C++ ABI (`_Unwind_Resume`, the `_Z*` mangled set).
