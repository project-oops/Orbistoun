# 2026-08-31 - Four plans crunched: testable layout, profiles, port reporting, vaddr provenance


Actioned the scoped plans from the payload work.

**Layout as a pure plan (Plan 1, highest leverage).** `plan_layout` in orbistoun-firmware decides
the libkernel layout - slot kind per export, overruns - as a pure, unit-tested function; the
worker only places what it returns. The collision that corrupted getpid is now a test, not a
runtime surprise, and `orbistoun-cli firmware` prints the layout. It immediately surfaced 89
collisions in the packed unimplemented region that were invisible before - a real follow-up
(stub-sizing), now visible.

**Console profiles (Plan 2).** `--profile ps5-cex-12.40` presents the measured reference machine
without hand-editing shell.toml; validated in the CLI, applied in the worker. Removes the
friction that was on every payload run.

**Port reporting (Plan 3).** `listen()` names the host address it bound, so a service announces
itself - the moment `pros check` waits for, previously silent.

**vaddr provenance (Plan 5).** A `confirmed`/`candidate` third column on the vaddr table, defaulting
to candidate; getpid and sceKernelWrite are confirmed; the layout verb shows it. Makes "which
vaddrs are behaviourally confirmed" machine-readable rather than a comment.

**Deferred (Plan 4):** the disc/digital Machine axis, until a guest is seen checking it - building
an axis nothing branches on is the speculation principle 12 forbids.

Surprise worth keeping: the layout verb, the first time it ran, reported 89 collisions. The
0x20-byte packing is tight enough that 18-byte unimplemented stubs and even 13-byte trampolines
overrun close neighbours. getpid (the anchor) is correct and the payload bails at the escape
before reaching them, so it is not blocking - but it is the next thing the layout work exposed,
and it would have stayed invisible without the pure planner.

