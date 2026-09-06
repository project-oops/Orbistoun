# 2026-09-02 - (/loop) R6: measurements become a work queue with a completion condition

> **Corrected 2026-09-02 (D480):** the sign-extension divergence recorded below is
> **withdrawn**. The leading `ffffffff` is obSCEne widening a C `int` through `int64_t`
> - `int second = scePthreadMutexTrylock(...)` then `(uint64_t)(int64_t)second` - not the
> console setting the top half of `rax`. A prototype returning `int` reads `eax` and never
> saw the other thirty-two bits, so those records could not have answered the question at
> all. Read at the width they were taken, every value is one orbistoun already produces.
> The width question D398 opened is still open and needs a register-level capture.

```
measurements recorded          0  ->  38   (27 constant, 11 the runs disagreed on)
hardware claims asserted       0  ->   6
outstanding, each with a why   0  ->  21
check() gates                  7  ->   8   (`also hardware`)
```

The load-bearing item. A function-shaped work list has no completion test - which is the
mechanism behind fourteen batches of "defensible slice, cannot tell it is unfinished, move on".
A measurement-shaped one does.

## The measure records carry what the `res` records could not

`OBS|measure|<check>|<subject>|<condition>|<observation>|<kind>` - and **`kind` is the
discriminator**. R5 could only quote a `res` value because nothing said whether it was an error
code or a timestamp. Here `code`, `hz`, `bytes` and `type` are constants and `ticks`, `us`,
`offset` and `handle` are not.

But the kind is not what decides it. **The runs decide it**: a measurement is `constant` when
every capture that took it agreed, which is an empirical answer rather than a judgement about
what `ticks` ought to mean. Eleven of the thirty-eight disagree and are kept, marked, and
unassertable.

`orbistoun-gen measurements` writes `crates/orbistoun-hle/data/hardware.toml` from 107 rows
across the two captures. It reuses `hardware.rs::fold` - the same agree-or-disagree logic R5
built - rather than a second copy of the rule, by packing check, condition and kind into the
grouping key with a unit separator, since a colon or a slash is a character the fields contain.

## The gate is coverage, not the assertions

`crates/orbistoun-service/tests/hardware.rs` requires every constant measurement to be **either
asserted against orbistoun or written into `OUTSTANDING` with the reason.** So:

- a new capture arrives as a **work list** rather than as a file nobody reads;
- each outstanding entry is one unit of work with an unambiguous completion condition - make
  orbistoun answer what the console answered, then move the id up into a test;
- deleting a test to make something pass fails the gate instead.

**Divergences are listed, not left failing.** A test red on purpose forever is not a queue, it
is a broken build people learn to ignore.

The gate was watched to fail before it was trusted, and it failed on real data without being
provoked: the first run named `direct-memory-query-flags:flags-0` and `flags-1`, which I had
not accounted for.

## Six claims that hold, checked by calling the code

Not assumed - called. `orbistoun_service::implementation_named` is new and public, because
every other route into these functions needs a loaded image, a thunk table and a relocation to
ask what one function answers. **R7's differential harness needs exactly the same door.**

- `sceKernelGetTscFrequency` answers `0x5f259b8e`, the measured value, and
  `sceKernelGetProcessTimeCounterFrequency` was measured as the same number under a second name.
- `kern.ostype`, `hw.ncpu`, `hw.pagesize` and `machdep.tsc_freq` answer the **byte widths** the
  console answered - 8, 4, 4, 8 - asked through `sysctlbyname`'s size half. The width is the
  claim rather than the value: a caller reading four bytes of an eight-byte answer reads a
  different number than was written, which is the failure D210 and D272 both record.

## Twenty-one outstanding, and what they say

The queue is more useful than the six passes. In order of what it exposes:

- **Nine on the return width** (D398). The console hands `0xffffffff8002_xxxx` back
  sign-extended; orbistoun builds every code with `u64::from(..as_raw())`, which zero-extends
  to `0x000000008002_xxxx`. D398 deliberately left the width unencoded because it "belongs with
  whichever shim returns it" - and these measurements say *which* shims:
  `scePthreadMutexUnlock`, `scePthreadMutexTrylock`, `sceKernelLoadStartModule`,
  `sceKernelDlsym`, `sceKernelDirectMemoryQuery`. Wants a decision before a sweep: per
  function, or the whole vendor family?
- **Three on the module loader** - the console answers real module handles (`0x15`, `0x14`) and
  `0x2001` for an already-resident one. Orbistoun's loader loads nothing, so it has nothing to
  hand back. That is the TitleOwn loader, now with three measured completion conditions.
- **Three on knobs orbistoun cannot source**, including `kern.version` at `0x2c` bytes, which
  D397 refuses on purpose rather than inventing.
- **Six** needing a harness or a mapping rather than a call.

## A number the gate printed that was wrong

The shell gate counted the outstanding list by grepping the Rust source and reported **26 for a
list of 21** - five of the reasons happen to begin with a digit, and the pattern matched them.

Caught by checking it rather than reading it, which is the whole habit. The count now comes
from the generated table, which has one line per measurement, and **the arithmetic is asserted
in the test instead**: claimed plus outstanding must equal the number of constants. A number a
gate prints is a claim like any other, and this is the one place that can check it.

## State

`cargo test --workspace` green - **116 suites, 1937 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean. The table regenerates byte-identical, so `also hardware` is
checking committed data against its generator rather than against a memory of it.

Nothing committed. 18:44 UK, and the day holds worklogs 292-319 and D466-D477.

**Next**: D478 - the differential provenance tier, which must be decided before R7 writes a
single record, because a FreeBSD result is `measured` about FreeBSD and still `assumed` about
the guest and `known_by` has no value meaning that. Then R7 itself, which now has its door.
