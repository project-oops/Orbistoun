# 2026-09-03 - (/loop) `.bss`, and a blind spot of our own making

```
tests   1991  ->  1991   (a report over numbers the loader already had)
```

The plan called one check "the cheap decisive one": is the null a `.data` the loader failed to
copy, or a `.bss` that is zero by design? It was cheap, it was decisive, and the answer needed
no new measurement at all - only for the loader to say what it already knew.

## The split, from numbers that were already there

`PlacedSegment` records `copied` and `zeroed` separately for every segment. That **is** the
`.data`/`.bss` split, and nothing printed it. Now it does:

```text
segment Il2CppUserAssemblies #4 0x480001d68000  0x194a6c copied  0x26e0dc zeroed  flags 0x6
```

Copied ends at `0x480001efca6c`. The faulting `rsi` is `0x480001f0c330` - **`0xf8c4` inside the
zeroed run**.

So the loader copied everything the file held. The global is zero because `.bss` is zero, and
the question stops being "what did placement get wrong" and becomes "what was supposed to write
this". Three hypotheses died over three ticks - missing constructors (D491), `module_start`
(D492), a failed `.data` copy (this) - and the placement path is now clear of all of them.

The caller is identified too: `+0x13d5f00` is `0x70` into the export at `+0x13d5e90`, which is
`0x9fdbed4fe0989d70` - one of the five imports the eboot binds into the module.

## And then the trace was empty

The obvious next question is which of those five the eboot calls first, and in what order. The
trace answers that. It did, until four worklogs ago:

```text
Il2Cpp entries in the trace: NONE
```

Before D489's binding, `0x6f8b9da539afc9af` showed up as **222 calls** carrying `il2cpp_init`
as its first argument - which is the only reason the resolver was ever found. Bound to a real
address, the guest calls it directly and no thunk sees it.

**Principle 7 says so in as many words**: *interception is linking, not hooking*. What gets
observed is exactly what resolves to a stub. Binding an import to real code is the right answer
and is also, in the same stroke, switching the instrument off for that call.

I did not think that through when writing D489, and the shrinking call count in worklog 339 -
10884 down to 2080 - was read as the guest taking a different path. It is that, and it is also
several hundred calls that still happen and are no longer counted. **The trace measures what
orbistoun answers, not what the guest does.**

## Which makes the next instrument non-optional

Every interesting call from here is one this build has deliberately stopped watching. So a
trace of bound calls is the next real piece of work, and it is a design question rather than a
feature: a thunk that forwards into the module would restore visibility and reintroduce exactly
the indirection principle 7 exists to avoid. Recorded in D493 rather than attempted at the end
of a session.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-343 and D466-D493.

**Next**: how to observe a bound call without undoing the binding. Then the `.bss` question it
is blocking.
