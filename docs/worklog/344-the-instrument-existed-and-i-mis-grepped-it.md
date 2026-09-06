# 2026-09-03 - (/loop) The instrument existed, and I mis-grepped it

```
tests   1991  ->  1991   (no code; three measurements and a correction)
```

D493 closed with *"the next instrument is a trace of bound calls, and it is not optional"*, and
framed it as a design question because a forwarding thunk would reintroduce the indirection
principle 7 forbids.

**There was nothing to build.** `ORBISTOUN_WATCHPOINT` has accepted `:x` since it was written -
a hardware execute breakpoint, one-shot, snapshotting registers at the hit, four at a time.
`watchpoint.rs` says so in its own module comment. D493 was written off the summary line
without reading the vocabulary underneath it, on the same day.

The rule that would have caught it was in the loop prompt, written by me the tick before:
*before adding a measurement, check whether something already holds the number.*

## Then I broke my own positive control

Armed on the faulting instruction - which the guest demonstrably executes - it appeared not to
fire. It fired. The two kinds of hit are worded differently:

```text
0x480001f0c338: touched after the access at 0x4800013d5eba …    <- data
execute breakpoint at 0x4800013dca44 … hit 1 time(s)            <- execute
```

`grep touched` finds one and misses the other. A working instrument read as broken, and the
next step would have been a defect report against code doing its job.

**A negative from a filter is a fact about the filter.** Same family as D490 and D492, and
three for three the cheap check would have saved the expensive conclusion.

## What it says once asked properly

Five imports the eboot binds into `Il2CppUserAssemblies`, under execute breakpoints:

| export | called |
|---|---|
| `0x9162adc3c86893df` | **twice** |
| `0x9fdbed4fe0989d70` | **once**, and faults |
| `0xc8275f216d188633` (`setenv`) | never |
| `0x6f8b9da539afc9af` | **never** |
| `0x00e9dafedfe399fe` | never |

**The resolver is never reached.** It showed 222 calls before binding, carrying `il2cpp_init`,
and that is why it looked like the centre of everything. With real code behind the other
imports the guest dies in its second call - long before asking for a symbol by name.

Those 222 calls described a path the guest only took *because* the imports answered
placeholders. A number measured under a broken condition measured the broken condition.

## And a data watchpoint over the whole run

```text
0x480001f0c338: touched after the access at 0x4800013d5eba, saw 0x0
0x480001f0c338: touched after the access at 0x4800013dca44, saw 0x0
```

Two accesses, both reads, zero both times, with the faulting read present as its own control.
**Nothing writes it.** The first read is `0x2a` inside `0x9fdbed4fe0989d70` - that export reads
the global, finds it empty, and calls the function that dereferences it regardless.

## What is open

`0x9162adc3c86893df` runs twice before any of it and does not touch that address. Whether it is
supposed to, whether the eboot should call something else first, or whether the platform fills
that memory at load, is unestablished - and the three obvious answers are dead already (D491,
D492, D493).

What is new is the ability to ask: execute breakpoints on module code work, four at a time,
with registers at the hit.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. fmt clean, identity
scan clean. No code changed this tick.

Nothing committed. The day holds worklogs 292-344 and D466-D494.

**Next**: what `0x9162adc3c86893df` does with its two calls - registers are already captured at
its entry, and its arguments are on the stack addresses the snapshot shows.
