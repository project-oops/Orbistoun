# D150 - Threads: the guest decides how many, the host decides how fast


**decided** · 2026-08-20

Worth writing down because the intuitive version is backwards.

The *guest* decides how many threads exist, by asking for them. The host's core count
decides how many run at any one instant. A title asking for thirty threads gets thirty
host threads on a four-core machine and on a thirty-two-core one; the second is faster,
not more parallel in any sense the guest can observe.

So there is **no minimum core count to enforce and nothing to refuse**. A slower machine
runs the same program more slowly, which is correct behaviour rather than a failure mode
worth detecting. Anything that "forced it to work in fewer threads" would be a scheduler
this project has no reason to write, and anything that added threads would be inventing
parallelism the program never asked for.

Two things the host's shape *does* change, and both are handled deliberately:

**What the guest is told about the machine.** `CpuTopology` reports the *target's* shape
by default - eight cores, seven usable - not the host's. A title asking how many cores it
has is asking about the machine its designers assumed, and answering with a thirty-two
core host is how a program ends up sizing a thread pool for a machine nobody tested it
on. `CpuTopology::host()` exists for the case where somebody genuinely wants the truth,
which is a developer measuring throughput and almost never a guest.

Stated assumption: the 8/7 figures have not been measured here.

**Affinity.** The guest can pin threads to particular cores. Honouring a mask literally
breaks the instant the host has fewer cores than it names; ignoring it silently discards
something the guest believed it was told. `AffinityPolicy` makes it a choice with three
positions, and **the request is recorded on the thread whichever one applies** - so a
title that turns out to depend on placement can be found rather than guessed at.

- `Observe` (default) - record it, let the host scheduler place the thread.
- `Map` - fold guest core `n` onto host core `n % host`. **Fold, not clamp**: clamping
  collapses every out-of-range core onto the highest one, which silently puts back
  together threads the guest deliberately separated.
- `Strict` - apply as given, refuse if the host cannot. Never a default; it exists so
  "did this title need exactly what it asked for?" is answerable.

`Observe` is the default deliberately, not because it is easiest. No title examined has
been shown to depend on placement, and a mapping invented before there is evidence is a
guess that later reads as a measurement.

All of it is `thread::Settings`, serialisable, reachable from `ServiceConfig` - because
principle 5 says rules live in data, and "how many cores does the guest think it has?"
has to be a file edit and a relaunch. The bisection loop is the only oracle most of this
project has, and anything needing a recompile to try is effectively untriable.

