# D066 - A guest that will not stop is a result, not a hang

**decided** · 2026-08-19

The 96 MB executable does not fault. It ran for **ten minutes** and was still going -
which sounds like success and is really a guest spinning on something that will never
happen, because every import it calls returns "unimplemented".

Killing it from outside loses the call trace, which is the only thing the run was for.
So the worker starts a watchdog **before** entering, and when the limit expires it
writes what the guest managed and exits with a status that means exactly that -
distinct from any fault code, because "still running when the clock ran out" and "died
on a bad pointer" call for completely different next steps.

The limit is per run (`--limit`, default 20 seconds; zero disables it). Not a constant,
because the right value differs between a quick sweep and a deliberate investigation.

**The result this produced immediately:**

```
the guest was still running after 10s; 299174391 import calls across 4 distinct imports
    299174387 calls ( 99.9%)  libkernel.prx::0x7dd1e10c2d2e7a04
            2 calls (  0.0%)  libc.prx::0x38af37e0078b6df0
            1 calls (  0.0%)  libc.prx::0x92f57c2dc704346f
            1 calls (  0.0%)  libkernel.prx::0x919bce0c4f7aefa4
```

**One function, three hundred million times.** That is a work list, and it is not
derivable from a static import dump - the dump says the executable needs 1,410 functions
and gives no hint that this one is the wall.

Percentages are integer tenths. The counts run past the point where an `f64` holds them
exactly, and a share that does not quite add up invites distrust of the whole report.

