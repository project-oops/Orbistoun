# D494 - Bound calls were always observable, and only two of five are called

**measured** - 2026-09-03 (execute breakpoints, and a correction to D493 written the same day)

D493 ended: *"the next instrument is a trace of bound calls, and it is not optional"*, and
called it a design question because a forwarding thunk would reintroduce the indirection
principle 7 forbids.

**No instrument was needed.** `ORBISTOUN_WATCHPOINT` has taken `:x` since it was written -
a hardware execute breakpoint, one-shot, snapshotting registers on the hit. Its own module
documents it. D493 was written after reading `watchpoint.rs`'s summary line and not its
vocabulary.

## And then a false negative of my own making

Armed on the faulting instruction as a positive control, it appeared not to fire. It fired.
The two hit reports use different words:

```text
0x480001f0c338: touched after the access at 0x4800013d5eba …      <- data
execute breakpoint at 0x4800013dca44 … hit 1 time(s)              <- execute
```

Grepping for `touched` finds the first and misses the second, so a working instrument read as
broken. The next step would have been a defect report against code that does its job.

Recorded because the shape recurs: **a negative from a filter is a fact about the filter.**
The same class as D490 (a label that could not have produced its own value) and D492 (validate
the tool before believing its "not found"), and it is now three for three that the check which
catches it is cheap and the one that skips it is expensive.

## What the instrument then said

The eboot binds five imports into `Il2CppUserAssemblies`. Under execute breakpoints:

| export | offset | called |
|---|---|---|
| `0x9162adc3c86893df` | `+0x1458ef0` | **twice** |
| `0x9fdbed4fe0989d70` | `+0x13d5e90` | **once** - and faults |
| `0xc8275f216d188633` (`setenv`) | `+0x1458f50` | never |
| `0x6f8b9da539afc9af` | `+0x13dbc80` | **never** |
| `0x00e9dafedfe399fe` | `+0x14860c0` | never |

**The resolver is never reached.** Before binding it showed **222 calls** carrying `il2cpp_init`
and `il2cpp_init_utf16`, and that is what made it look like the centre of the problem. It is
not: with real code behind the other imports the guest dies in the second call it makes, long
before it would ask for a symbol by name.

So the 222 calls were an artefact of the guest running a path it only took *because* those
imports answered placeholders. A number measured under a broken condition described the broken
condition.

## And nothing writes the global

Separately, a data watchpoint on `0x480001f0c338` for the whole run:

```text
touched after the access at 0x4800013d5eba, saw 0x0
touched after the access at 0x4800013dca44, saw 0x0
```

Two accesses, both reads, `0x0` both times, and the faulting read present as its own control.
**Nothing writes it.** The first read is inside `0x9fdbed4fe0989d70` itself, `0x2a` in - so that
export reads the global, finds it empty, and calls the function that dereferences it anyway.

## What is open

`0x9162adc3c86893df` runs twice before any of this and does not write that address. Whether it
is meant to, whether the eboot is meant to call something else first, or whether the platform
fills that memory at load, is not established - and the three obvious answers are already dead
(D491 constructors, D492 `module_start`, D493 a failed `.data` copy).

What *is* now available is the ability to ask: execute breakpoints on module code work, four at
a time, with registers at the hit.
