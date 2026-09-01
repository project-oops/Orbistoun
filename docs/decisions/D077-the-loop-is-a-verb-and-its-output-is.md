# D077 - The loop is a verb, and its output is durable

**decided** · 2026-08-19 · prompted by the user

The project is built around one cycle - execute a guest, read what it wanted, implement
the frequent ones, execute it again - and until now that cycle existed only in my head.
There were verbs (`check`, `names`, `run`) and a roadmap, but nothing said what order to
run them in, what to read from the output, or how often. Reconstructing it meant reading
the decision log.

Now `./orbistoun.sh sweep` is one turn: refresh names, run every module available, rank
what they actually called. A documented sequence drifts from what people run; a script
cannot.

**The trace was volatile, which was the worse half.** A call trace went to standard error
and died with the process that produced it - after a run that could take ten minutes.
Traces are now written as JSON to the data directory, keyed by module so a sweep leaves
one file per title, and `orbistoun-cli worklist` totals across all of them.

**Persisted on every path, including the fault handler.** A guest that faults has still
said what it wanted, and that was exactly the case losing its data - three of four
modules recorded nothing. Writing from a fault handler allocates, which the fault
*message* deliberately does not; the trade is deliberate, because the handler runs in
ordinary user context on a thread that faulted on a **guest** pointer, so the allocator
is not the thing that broke. If it ever deadlocks, it deadlocks a process that was about
to die. Losing the only output of a ten-minute run is the worse outcome.

**Totals are keyed by label, not by stub index.** An index is per-module - index 260 is a
different function in every title - so summing by index would produce confident nonsense
the moment a second module was involved.

**Two bugs found by wiring it up.**

- The worker ran with no paths configured, so nothing would have been written anywhere
  regardless. It is the only process that ever sees a trace.
- Databases were written with `suffix_hex: ""` whenever the shipped default was used,
  which makes a database **unloadable** - so every trace fell back to printing hashes,
  which reads as the name search having failed rather than as the file being unusable.
  The suffix actually hashed with is now written, not the argument the user typed.

The result, across four commercial executables: **54 distinct imports, ranked**, led by
`sceKernelDirectMemoryQuery` at 99.9% of all calls. That is a work list.

Written up in [WORKFLOW.md](../WORKFLOW.md), including what each outcome means and what to
do about it.

