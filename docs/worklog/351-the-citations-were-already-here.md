# 2026-09-03 - (/loop) The citations were already here, and R9 was already closed

```
published        348  ->  362
assumed          265  ->  251
differential     211  ->  235 cases
R9                     closed (verified, not done)
```

Crunching the no-new-capture list. Two of its five items turned out to be smaller than stated,
and one of those corrections is mine to make.

## Fourteen entries were bookkeeping, not research

`math.rs` adds ten float functions in one block from one standard. The first line names it:

```rust
// `acosf(x)` - ISO/IEC 9899 7.12.4.1.
// `asinf(x)` - 7.12.4.2.
```

`acosf` is **published** with that citation. `asinf` is **assumed**, with `7.12.4.2.` sitting in
its `purpose` field and the note *"cites no published specification"* - which was false. One
block, one generator, six characters of difference.

Fixed in the comments rather than in the generator: teaching it to accept a bare `7.12.4.3.`
would be the provenance-manufacturing its own guard exists to prevent. Twelve comments now name
the standard - better documentation anyway, since a reader of one line should not have to scroll
to a header. `strtoimax`/`strtoumax` said `C99`, a name the list does not carry; spelled
`ISO/IEC 9899` they cite their exact clause.

**Verified by deleting all fourteen and letting the generator rewrite them.** All fourteen came
back `published`; thirteen with the exact sub-clause. `hypotf` came back with only the bare
standard name because prose followed the clause on its line, so that line was split and it
regenerated exact. D501.

## And the correction I owe: the rest is not citation work

I said a large share of the 265 had a documented analogue waiting. **That was wrong.** Ninety-
nine are `libScePosix`, and their assumption reads:

> That the POSIX name and `scePthreadMutexLock` are the same behaviour rather than merely
> similar. Unmeasured.

No standard settles that. POSIX says what `pthread_mutex_lock` does and nothing about the
vendor's export of that name - and D468 is this project already paying for that exact
confusion once, with the ctype tables. Relabelling them would have been that mistake ninety-nine
times.

The remainder are Dinkumware CRT internals, Itanium-ABI throw helpers, and vendor `libkernel`
functions with no analogue. **The assumed count is close to honest.**

## R9 was closed, and nobody had checked

`provenance_faults` carries `learn`'s rule and is wired into `check()`; a test asserts it empty
on every run. The second half - *every implementation has a knowledge entry* - is also done:
**694 entries against 475 bound implementations**, and the five names my first query flagged as
missing were test fixtures (`abi`, `loader`, `mem`, `thunk`, `worker`), not guest functions.

Fifth stale work item this week, after `/dev/random`, the differential's "missing" cases,
D488's note on the verdict, and Phase 0d.

## Also measured, not acted on

The 95 outstanding hardware measurements are **not** mostly actionable. Thirty-nine of them are
`106-encoder/*` - "whether a symbol resolves inside a video-encoder library" - and orbistoun
implements no encoder subsystem, which principle 6 puts well after the address space and
threads. They are correctly parked, not neglected.

## And the differential covered everything except the two most-called functions

**211 -> 235 cases.** Checking the case list properly this time rather than trusting the
plan's list, the gap was not exotic: **`strlen` and `memcpy` had no case between them**, and
they are the fifth and sixth most-called functions in the whole recorded corpus at 89,811 and
97,664 calls. A differential covering `strtoumax` and not `memcpy` is measuring the wrong end.

Added with `abs`/`labs`/`llabs` and `atoi`/`atol`/`atoll` - the latter being `strtol` with the
error reporting removed, which is exactly why they need their own cases rather than being
assumed to follow it. `INT_MIN` is deliberately absent from the absolute values: negating it is
undefined, and a case for it would record one implementation's choice as the contract.

All 235 agree. Watched failing by corrupting one byte of the `memcpy` window, after the
`run_scalar` refactor changed the dispatch again.

**Three of my own mistakes, caught by reading the output rather than by the gate:**

- A shell heredoc collapsed nine `
` escapes into real newlines, so the reference did not
  compile - **and the redirect had already truncated the committed file to zero cases.** Write
  to a temp, verify, then install; the script now refuses to install a short run.
- `"ab"` is not a high byte followed by `b`. A hex escape in C has no length limit, so
  that is the single character `0x80b` truncated - `strlen` measured 2. Split the literal.
- Before that, the same escape had been typed as a character and arrived as two bytes of UTF-8,
  measuring 4. The case is named "embedded-high-byte" and was about neither.

## State

`cargo test --workspace` green - 117 suites, **1992 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-351 and D466-D501.

## Scoreboard for the no-new-capture list

Five items were named. Measured, three of them are smaller or already closed:

| item | stated | actual |
|---|---|---|
| 95 hardware measurements | a work queue | 39 are encoder symbols with no encoder subsystem; `130-layout` needs a capture under a different application category; `kern.sdk_version` has no value to source (D397). Correctly parked. |
| 265 assumed | "a large share" citable | **14 were.** The rest are name-identity assumptions no standard settles. |
| 53 unnamed imports | derivable | **The harvesters are exhausted** - a 2.6-billion-candidate sweep found none. A documented plateau, not pending work. |
| more differential cases | yes | **211 -> 235**, and the gap was `strlen` and `memcpy`. |
| R9 | to do | **already closed**, both halves. |

**Next**: the differential is the one item with headroom left - `strtok_r`, `strtof`,
`sprintf`/`vsnprintf` and the wide-character functions are uncovered, the last needing a wide
text encoding in the record format.
