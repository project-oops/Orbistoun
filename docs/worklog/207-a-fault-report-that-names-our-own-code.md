# 2026-08-29 - A fault report that names our own code


The `-1` chase produced two things worth keeping and did not produce a cause.

**The reporter names where in orbistoun's own code a fault landed** (D380). It could not
before: "inside libc::vsnprintf" names the last import called, which is an attribution rather
than a location. The binary carries no symbols on this toolchain - but a `GuestFn` is a
function pointer and this project has a table of them, so the nearest preceding implementation
is one sorted lookup away. The distance is printed with the name, which is what keeps it
honest.

Its first answer: `posix_sigdelset+0x391c1` - two hundred kilobytes past the last
implementation, so the fault is **not in an implementation at all**. That is a fact the report
could not previously state.

**The renderer follows a pointer only where the run mapped one.** The dumper always checked;
nothing else did, on the principle that a guest's bad pointer should fault as it would have on
the machine. That is right for a pointer the guest computed and wrong for one it never set -
and a `%s` reading an overflow area that holds no arguments is the second kind. Guarding null,
then all-ones, then a third value is a losing game against arbitrary stack contents, so `%s`
now asks the same question the dumper asks. It also removes an invention: `(bad pointer)` was
text this project made up appearing in a guest's output, and `(unmapped)` is a statement about
a range the run published.

**And the fault is still unexplained**, after eight eliminations. It is in support code, it is
reached from the `vsnprintf` path, and pinning it needs a debugger or a symbolised build.
Every change made while chasing it stands on its own merits and none of them was the cause.

Also: stole a doc comment inserting above a documented item, for the fourth time this session.
The gate caught it, as it has each time.

