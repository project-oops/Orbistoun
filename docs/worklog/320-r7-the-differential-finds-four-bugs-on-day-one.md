# 2026-09-02 - (/loop) R7: a live oracle at last, and it found four bugs in twenty-four cases

> **Corrected 2026-09-02 (D480):** the sign-extension divergence recorded below is
> **withdrawn**. The leading `ffffffff` is obSCEne widening a C `int` through `int64_t`
> - `int second = scePthreadMutexTrylock(...)` then `(uint64_t)(int64_t)second` - not the
> console setting the top half of `rax`. A prototype returning `int` reads `eax` and never
> saw the other thirty-two bits, so those records could not have answered the question at
> all. Read at the width they were taken, every value is one orbistoun already produces.
> The width question D398 opened is still open and needs a register-level capture.

```
reference cases recorded        0  ->  24   (glibc 2.39, committed and regenerable)
divergences found               -      4
divergences fixed               -      4
check() gates                   8  ->   9   (`also differential`)
tests                        1937  -> 1944
```

The first oracle this project has had that is both live and unbounded. Hardware answers one
machine's worth of questions per run and there is no console; a reference C library answers as
many as you write cases for.

## FreeBSD is not reachable, and the reason was not the one expected (D479)

RAM was the assumed constraint. **It is not**: 31.9 GB with 14.2 free. The actual ones:

- Hyper-V is installed but **not permitted** - `Get-VMHost` answers "You do not have the
  required permission", so it needs an elevated session.
- multipass publishes **Ubuntu images only**.
- No qemu, no VirtualBox, no vagrant, and a FreeBSD image is a multi-gigabyte third-party
  download that is not mine to start unasked.

**WSL2 works** - glibc 2.39, gcc 13.3, no admin, no download. D478 had already worded the tier
as "a published implementation of the same interface" with `needs_citation` true, so running
against glibc and naming it is that clause being used rather than a rule being bent.

What that buys is conformance to ISO C and POSIX, which is most of the mechanical surface.
What it does not buy is any statement about the console - and D468 is this project watching
that exact gap open, when the FreeBSD-*documented* ctype layout turned out not to be the
platform's.

## The shape that stops a differential quietly becoming nothing

`tools/differential/reference.c` emits **the inputs it used** alongside its results, and the
checker rebuilds each call from those. A differential where each side keeps its own copy of the
cases is one that drifts until it compares two different things and reports agreement. With one
list there is nothing to drift.

Three things are recorded per case - return value, `errno`, and the out-parameter. The third is
the one that matters here: the failures this project has actually had are a semaphore handle
written eight bytes wide (D210) and an attribute getter clobbering the caller's loop counter
(D272). A return value that matches while the wrong bytes land next door is precisely what a
return-value-only differential misses.

## Four bugs, in the first twenty-four cases

Twenty agreed. The four that did not were all **standard-mandated behaviour orbistoun got
wrong**, and all four are now fixed and confirmed by re-running:

### Conversions wrapped instead of saturating

`strtoul` on twenty-three nines answered `0x2c7e14af67fffff`. ISO C requires `ULONG_MAX` and
`ERANGE`. Same for `strtoull`, and `strtol` underflow answered a wrapped value where `LONG_MIN`
is required.

**The wrong answer was the dangerous kind**: not obviously broken, but a plausible number in
range, so a caller range-checking the result would accept it and carry on.

The cause was a shape problem rather than a missing clamp. `parse_int` accumulated into a
signed 128-bit value and the caller truncated to its width, so no limit was ever applied. It
now returns the **magnitude and the sign separately**, and each conversion applies its own
limits - which it has to, because `strtoul("-1")` is `ULONG_MAX` in ISO C, so this is not a
clamp into range and a naive one would answer zero.

**`atoi` deliberately still truncates.** ISO C 7.22.1.2 leaves it undefined where `strtol` is
required to saturate, and a test already pinned the truncation. Clamping it too was my first
patch; the existing test caught it.

### `snprintf` with no room answered zero

ISO C 7.21.6.5: with a size of zero nothing is written and the return is still the length the
output *would* have needed - which is the whole reason a caller passes zero, to size a buffer
before allocating it. Orbistoun returned zero, telling that caller it needed no space at all.

**Two existing tests had pinned the bug**, which is the part worth keeping. One was named "a
zero-sized destination is left alone" and asserted both that the buffer was untouched (right,
and still asserted) and that the return was zero (wrong). A test can only protect the
behaviour somebody thought of, and nobody had thought of this one until another implementation
was asked.

## The gate, watched to fail in both directions

`also differential` rebuilds the reference program and diffs its output against the committed
run - the same property `also tables` enforces, because a recording that no longer matches its
generator is a comparison against history. It warns rather than failing where no reference
library is reachable, since a step that cannot run has not passed.

Its first version **warned every time**, because it handed this shell's `/c/...` path to WSL,
which means nothing there; the build found no source and the warning read as "no library
available". `pwd -W` gives the Windows spelling that `wslpath` can translate. A gate that
always warns is a gate nobody reads, so that was a real failure and not a cosmetic one.

Then it was made to fail deliberately: a doctored value in the committed run produced the diff
and a non-zero exit, and restoring it went back to `24 cases, matching`.

## State

`cargo test --workspace` green - **117 suites, 1944 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean. The reference run regenerates byte-identical.

Nothing committed. 19:35 UK, and the day holds worklogs 292-320 and D466-D479.

**Next**: more cases, which is now the cheapest work in the project - every case written is a
question answered by something other than a guess. The obvious seams are the rest of the
`str*`/`mem*` family, the `_s` bounded forms, and `qsort`/`bsearch`, whose comparison callback
means the reference and orbistoun must agree about calling back into guest code. BSD-only
functions (`strlcpy`, `strnstr`, the `_np` family) have no glibc reference and stay outstanding
until a FreeBSD box exists.
