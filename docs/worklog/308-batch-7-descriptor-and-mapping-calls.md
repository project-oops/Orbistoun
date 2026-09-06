# 2026-09-02 - (/loop) Bulk port batch 7: scatter/gather, sync, and the ones refused instead

```
documented   714 needed, 374 missing   ->   715 needed, 366 missing
```

Nine implemented, five refused. The refusals are the more interesting half.

## In

`readv`, `writev`, `preadv`, `pwritev`, `creat`, `fsync`, `fdatasync`, `getpagesize`,
`madvise` - all in `orbistoun-fs/src/posix.rs`, beside the file calls they belong with, with
delegation rows in `orbistoun-posix`.

Three details worth writing down:

- **The vector calls answer bytes, not buffers**, and **stop at the first short transfer**.
  Continuing past a partial buffer would report a total the file never delivered - the test
  writes three buffers and asserts `11`, then reads them back to prove the order, because a
  version answering `3` (the buffer count) also "passes" a test that only checks non-zero.
- **A zero-length entry is skipped, not treated as the end.** The obvious loop stops on it.
- **`fsync` really flushes.** It needed a new `descriptor::sync` calling `sync_all`, because
  the entire point of the call is that a caller learns its bytes have landed - answering
  success without asking the operating system gives the assurance and none of the substance.
  `fdatasync` flushes metadata too, which is *more* than it promises and therefore conforming;
  the reverse would not be.

`getpagesize` answers `GUEST_PAGE_SIZE` rather than the host's page size: a guest rounding an
allocation must get the number this emulator's mapper will actually use.

`madvise` is the rare case where doing nothing **is** the specification - POSIX says the advice
is not binding and an implementation may ignore it - so it answers success without acting, and
the comment says which of those two things it is.

## Refused, and why each

- **`flock`** - advisory locking whose host semantics differ from POSIX's. The tempting no-op is
  the same silent hazard the Dinkum locks were (worklog 303): correct-looking until two things
  share a file.
- **`msync`** - would need to know whether a mapping is file-backed, which this layer does not
  model. Answering success for a mapping that was never flushed is exactly the assurance-without-
  substance `fsync` avoids.
- **`getrlimit`** - has no answer here that is not invented. There are no resource limits to
  report, and a made-up `RLIM_INFINITY` is a number a guest would size an allocation from.
- **`getdents` / `getdirentries` / `_getdirentries`** - need the exact `struct dirent` layout the
  platform uses. FreeBSD publishes its own, but this is where FreeBSD-derived stops being a
  guarantee (D468 found the C runtime is Dinkum-derived, not FreeBSD's), and a wrong field
  offset produces plausible directory entries. `opendir`/`readdir` already work for guests that
  use them.
- **`getsockopt` / `setsockopt`** - implementable, but properly: each option needs its own
  mapping and the honest version is a per-option table, not a blanket success. A batch of its
  own rather than four lines here.

## State

clippy `--tests` clean, fmt clean, orbistoun-fs and orbistoun-posix tests pass (including the
three delegation guards, which are self-adjusting and caught nothing this time because the
arities were checked first), identity scan clean, nothing committed.

**Next**: the remaining ~250 POSIX names, then `getsockopt`/`setsockopt` as their own batch. The
structural piece after that is the TitleOwn loader, which is unbuilt code rather than an unknown.
