# D224 - Mapping the faulting address moved the wall, and the reading was wrong


**decided** · 2026-08-24 · the seventh elimination left only one reading

> **Superseded in part by D226.** The diagnostic and everything it exposed stand; the
> conclusion - that `0xfffe0` was an address the guest expected mapped - does not. Watching
> the region one run later showed the guest writing an arena header there at
> `size - 0x20` from a base of zero, so the address was wrong and the mapping only gave a
> wrong pointer somewhere to land. Read D226 with this.

`ORBISTOUN_MAP=<addr>[+len]` reserves a region of guest address space before entry, for one
run.

### Why this was the question left standing

Seven things had been eliminated at `image+0xafc959` - the stub return, unwritten stack,
unwritten heap, two argument targets, `memalign`, and an unfilled slot in the object the
fault carries. Every candidate for a *base that got lost* was gone.

Which left the reading nobody had tested: **that no base was lost, and the address was
right.** A fault reported as *"an address in no region this run mapped"* is exactly as
consistent with a missing mapping as with a bad pointer, and every diagnostic built so far
assumed the second.

### It worked, and it is the first confirmation in two days

```
baseline               write to 0xfffe0   image+0xafc959
0xf0000+0x10000        write to 0x0       image+0xafca2e   FURTHER
```

Two hundred and thirteen bytes further into the same function. `0xfffe0` was a legitimate
address the guest expected to be mapped, and the emulator was not mapping it.

The new fault is a genuine null - so there *is* a forgotten-slot problem, and it is the
**next** one rather than the one two days went into.

**This is a diagnostic and must stay one.** Mapping memory until a fault stops happening is
precisely the plausible-output trap principle 3 refuses; what makes it legitimate is that it
is ephemeral, recorded in the run conditions, and answers a question instead of hiding a
symptom. What the guest is entitled to expect at that address is still unknown, and the real
fix is knowing that rather than reserving a megabyte because it helps.

### Two things fell out of it

**The address-space layer refused a misaligned reservation and said why**: *"requested
0xff000, kernel returned 0xf0000"* - the host's 64 KiB allocation granularity. A layer that
had quietly mapped what the kernel offered would have produced a run that reported success
and answered a different question.

**The progress verdict was mislabelling itself.** `Verdict::Further` fires either for more
of the interface *or* for the same interface with the fault further along, and its summary
said **"reached imports it could not reach before"** for both. This run hit the second case:
23 imports before and after, the fault 213 bytes on. So the single line this project steers
by printed a falsehood directly above its own `(+0) distinct` count.

It now says *"executed code it could not reach before"*, which is true of either cause and
is how D080 states the measure in the first place. Pinned by a test built from exactly this
run's numbers.

That is the fourth thing this session that reported something its measurement did not
support - after the `found_by` gate, the ceiling comparison, and the readable window.

