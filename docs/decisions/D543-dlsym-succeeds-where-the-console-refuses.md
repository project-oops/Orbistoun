# D543 - `dlsym` succeeds where the console refuses, and the reason written down was not the reason

**decided** - 2026-09-04

First tick on a new axis: `crates/orbistoun-service/tests/hardware.rs`, which the loop notes name
as the second place to look for work and which had not been opened once in this run.

Sixty outstanding measurements, and reading all sixty - not the top of the list - collapses them
to **sixteen distinct blockers**. Fourteen need a capture this project does not have. One is a
coincidence it declines to claim (the console's L1 line size is `0x40` and so is this host's,
read through the same `cpuid` the guest executes natively). One was wrong.

## The reason written down had stopped being true, and had never been the reason

```text
110-modules/symbol:sceKernelDlsym:memcpy
  "0x80020003 is errno::NO_SUCH; needs a loaded module to resolve against,
   which the loader does not yet provide"
```

The loader has provided one since D517 added guest exports. But that was never the mechanism:
**orbistoun does not fail this call. It succeeds.**

obSCEne's `110-modules/symbol` loads libkernel - handle `0x2001`, a value three separate
measurements agree on - and asks it for `memcpy`. The console answers ESRCH: libkernel does not
export it. Orbistoun publishes every function it implements in one flat by-name table
(`symbols::resolvable()`) and `dlsym` looks a name up there **without consulting the module
handle at all**. So it answers `0` and writes an address into the guest's out-parameter, for a
symbol the module the guest named does not have.

Measured rather than read - the runtime table is installed during a load, so a test binary sees
`dlsym` answer the placeholder and would have told a comfortable story. Standing a one-entry
table in for it:

```text
without thunks   dlsym(0x2001, "memcpy") = 0x7fff0001   (this project's placeholder)
with thunks      dlsym(0x2001, "memcpy") = 0x0, out = the address
the console      dlsym(0x2001, "memcpy") = 0x80020003   (ESRCH)
```

Three answers for one call depending on what is installed, and the entry described none of them.

## The trap underneath it

Orbistoun **already answers `0x80020003` from `dlsym`** - for a *negative* module handle, the
branch obSCEne's `060-module/dlsym-rejects-bad-handle` covers (D366). A test written to claim
this measurement with an invalid handle passes, in green, having exercised a branch with nothing
to do with what was measured.

That is check 11 and check 19 in one place: the new claim shares a replay path with an old one,
and the branch that produced the matching value was not the branch under discussion. It is also
the reason this got a test rather than only a corrected sentence - the next person to read
"expected `0x80020003`, orbistoun answers `0x80020003`" will reach for the easy claim.

## Recorded, not fixed

The fix is a per-module export list for the platform's own libraries, and nothing lawful here
provides one. Inventing which symbols libkernel exports would not be a wrong answer, it would be
a fabricated one - and unlike a wrong answer it would look authoritative.

Nor is the current behaviour obviously worse for a guest: a title asking for `memcpy` and getting
it keeps running. What is wrong is that a guest asking *whether* a symbol exists is told yes by
something that never looked. So it goes in `OUTSTANDING` with the mechanism written down, which
is what that list is for.

`tests/dlsym_divergence.rs` pins it, green on purpose - this file's doctrine is that a
permanently red test is a build people learn to ignore. What it buys is noticing when the
divergence stops being the one that was written down: made to fail by making `dlsym` consult the
handle, which is exactly the change that should send somebody back to the entry.

## The general shape

Second time in two ticks that a stated blocker was wrong about its own mechanism, and the third
in this run that reading a thing beat trusting what was written about it. The pattern is
specific enough to name: **an entry recording why something cannot be done is written once, at
the moment of giving up, and nothing re-derives it afterwards.** The measurements are
regenerated from captures; the reasons beside them are not.
