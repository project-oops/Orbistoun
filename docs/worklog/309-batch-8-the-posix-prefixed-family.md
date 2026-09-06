# 2026-09-02 - (/loop) Bulk port batch 8: the `posix_`-prefixed family, 69 in one go

```
documented   715 needed, 366 missing   ->   715 needed, 297 missing
```

The largest single drop so far, and it came from **reading the list properly** rather than
writing more code.

## The finding

`libScePosix` exports a whole family that is an existing POSIX function with `posix_` in front -
`posix_open`, `posix_close`, `posix_pthread_create`, `posix_mmap`. They are **142 of the 249**
names still missing, so what happens to them decides most of the remaining work. Sixty-nine have
an unprefixed twin already implemented, and are now delegated to it (D475).

Crucially the assumption this rests on was **already recorded** before this batch - the knowledge
file's existing `posix_*` entries say "semantics follow the POSIX analogue of the same name;
nothing on the target has confirmed them, and the error codes are placeholders". So this acts on
a recorded assumption rather than inventing one, and stays `assumed` rather than `published`.
What is genuinely unconfirmed is the **failure convention** - `-1`/`errno` versus a negative
vendor code - which is the first thing to suspect if a title misbehaves on an error path.

## Two guards did their job, and one needed sharpening

**The delegation table maps a POSIX name to the function that implements it**, and many
unprefixed twins are *themselves* aliases: `close` is a row pointing at `sceKernelClose`, not an
implementation. So pointing `posix_close` at `close` named nothing - and
`every_delegation_resolves_to_a_real_implementation` failed immediately, which is exactly what it
is for.

It then could not say **which** of 160 rows was broken - only that the counts differed. Fixed:
it now names the offending `alias -> target` pairs, which is the same "a message naming a cause
must come from the branch that determined it" the principles already require, applied to a test.
With that, the six stragglers were obvious in one run: `cargo fmt` had split those rows across
lines with a trailing comma and my repointing pattern had not matched them.

**The libc declaration gate** then caught the batch-7 additions: `creat` and friends live in the
filesystem layer, flow into libc's implementation list, and are declared in `libScePosix`. That
gate is deliberately a **named list rather than a blanket exemption** - "the guard is only worth
having if a new one has to be argued for" - so each was added with the argument written beside
it rather than the check being loosened.

## The 73 left

The other prefixed names have no unprefixed twin implemented either, so there is nothing to
delegate to. Ordinary future work, counted in the same gap.

## State

clippy `--tests` clean, fmt clean, libc/fs/posix/hle tests pass, identity scan clean, nothing
committed.

**Next**: the remaining 297, of which 73 are the prefixed family awaiting their twins. Then
`getsockopt`/`setsockopt` as their own batch. After that the TitleOwn loader, which is unbuilt
code rather than an unknown - and the obSCEne differential, which is the only check for
"implemented but wrong" and has never been run against any of this.
