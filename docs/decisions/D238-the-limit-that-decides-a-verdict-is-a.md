# D238 - The limit that decides a verdict is a call budget; the clock stays as a backstop


**decided** · 2026-08-25 · the gap D181 and D194 both recorded and deferred

The guest limit was wall-clock, so the duration was fixed and the **call count varied** -
and the call count is what every `FURTHER`/`same`/`BACK` verdict is read off. Three
identical runs of PPSA04263 returned 77.5M, 75.8M and 87.6M calls, a 13% spread with no
change to the build. A verdict between two of those measures the machine.

That was tolerable while verdicts were incidental. It stops being tolerable the moment the
loop becomes "apply an answer, re-run the corpus, read the verdict", because then the
measurement is least trustworthy exactly where it is load-bearing - and a false `FURTHER`
bought by machine noise is worse than no verdict at all.

A budget inverts the pair: the **count is fixed and the duration varies**. Three runs after
this change returned 20,000,000 calls, 20,000,000 and 20,000,000.

### It does not replace the clock

A call budget cannot stop a guest that stops calling imports. An idle loop waiting on
something that never happens makes no calls, never reaches a budget, and hangs - which is
the exact failure D066 built the clock for. The two answer different failure modes, both are
installed, either may fire, and the **exit status says which**: "ran out of clock" and "made
the calls it was allowed" call for different next steps and must not read alike.

### On the call path, deliberately

A watcher thread polling the call counter would cost nothing on the call path and stop at
*about* the budget - 20,000,137 one run and 19,999,882 the next. That is the nondeterminism
this exists to remove, reintroduced in a smaller font. The check is one relaxed load and a
comparison against `u64::MAX` on any run with no budget, so the ordinary path pays a
predictable branch and no table lookup (principle 9).

The stop is a `fn` pointer the worker installs rather than logic here: writing a trace means
collecting, persisting and summarising it, all of which live above this crate.

### The default

Twenty million. The busiest title doing real work makes **1,735** calls; the one this bounds
spins on a single import and made **149 million**. Four orders of magnitude above the first
and well below the second, so it stops a runaway at a fixed number and cannot truncate a
legitimate run. Runs of that title are also seven times shorter, which was not the point but
is worth having.

`Conditions` records the budget beside the clock, and a change to either is reported as a
difference - because a guest stopped at ten million and one stopped at twenty reaches less
for a reason that has nothing to do with the build.

