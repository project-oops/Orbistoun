# 2026-09-04 - (/loop) `dlsym` succeeds where the console refuses

```
60 outstanding measurements -> 16 distinct blockers; 14 need a capture, 1 is a coincidence, 1 was wrong
suites 128   clippy/fmt/identity clean on both repos
```

Twenty-ninth cron tick. New axis, and one the loop notes have named as pick #2 all along:
`crates/orbistoun-service/tests/hardware.rs`, which had not been opened once in this run.

**Reading all sixty, not the top ten** - D542's lesson about ranked queues - collapses them to
sixteen blockers. Fourteen need a capture nobody has. One is a coincidence the file declines to
claim: the console's L1 line size is `0x40` and so is this host's, read through the same `cpuid`
the guest executes natively, so claiming it would be claiming an accident.

## The one that was wrong

```text
110-modules/symbol:sceKernelDlsym:memcpy
  "needs a loaded module to resolve against, which the loader does not yet provide"
```

The loader has provided one since D517. But that was never the mechanism: **orbistoun does not
fail this call, it succeeds.**

obSCEne loads libkernel - handle `0x2001`, agreed by three measurements - and asks it for
`memcpy`. The console answers ESRCH: libkernel does not export it. Orbistoun publishes every
implementation in one flat by-name table and `dlsym` looks a name up there **without consulting
the module handle**, so it answers `0` and writes an address for a symbol the named module does
not have.

Measured, not read - the runtime table is installed during a load, and a test binary would have
told a comfortable story:

```text
without thunks   dlsym(0x2001, "memcpy") = 0x7fff0001   the placeholder
with thunks      dlsym(0x2001, "memcpy") = 0x0, out = the address
the console                              = 0x80020003   ESRCH
```

## The trap underneath

Orbistoun **already answers `0x80020003`** from `dlsym` - for a *negative* handle (D366). A test
claiming this measurement with an invalid handle passes, green, having exercised a branch with
nothing to do with what was measured. Checks 11 and 19 in one place, and the reason this got a
test rather than only a corrected sentence.

## Recorded, not fixed

The fix needs a per-module export list for the platform's own libraries and nothing lawful here
provides one; inventing which symbols libkernel exports would be a fabricated answer rather than
a wrong one. `tests/dlsym_divergence.rs` pins it green-on-purpose - the file's doctrine is that a
permanently red test is a build people ignore - so that the divergence *changing* is what fails.
Made to fail by making `dlsym` consult the handle.

Decision: [D543](../decisions/D543-dlsym-succeeds-where-the-console-refuses.md).
