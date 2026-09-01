# D275 - A clock that does not advance reads as a sleep that returned instantly


**decided** · 2026-08-25 · an assumption suspected, and cleared by measuring

`120-measure/sleep-fidelity` reported *"a sleep returned far sooner than asked"*, and the
first suspicion was the microsecond unit assumed for `sceKernelGetProcessTime` (D256).

It was not. The check measures with `sceKernelReadTsc` and `sceKernelGetTscFrequency`,
neither of which was **declared at all** - so the counter never advanced, elapsed read zero,
and every sleep looked instantaneous. A stub answering a constant is indistinguishable from
a clock that has stopped.

Implementing them gave a cross-check worth keeping:

```
sceKernelReadTsc         2,759,700 ticks (ns) = 2.76 ms
sceKernelGetProcessTime      2,792 ticks (us) = 2.79 ms
```

Two independent clocks agreeing to within a percent. That does not establish the platform's
unit - both are ours - but it does show the assumption is coherent rather than arbitrary,
which is more than it had before.

The counter runs at a nominal billion ticks a second so the arithmetic is exact and two
machines are comparable. **The target's real frequency is a different number**, and a title
deriving a frame budget from it would pace itself wrongly with nothing in a trace saying so.

`sceKernelIsStack` answers from the span the worker actually mapped, and **false** when
nothing has told it - a wrong yes is worse than a refusal, because a guest told a static is
stack memory may free it. `sceKernelLoadStartModule` refuses everything, including modules
that exist, because orbistoun places one executable at load and cannot bring another in;
answering a handle would tell a guest a library it is about to call is present.

