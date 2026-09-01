# 2026-08-19 (late) - The loop, made runnable


The development cycle existed only in my head. Now it is `./orbistoun.sh sweep` and
[WORKFLOW.md](../WORKFLOW.md). D077-D078; 24 crates, 273 tests.

### Surprises

- **The call trace was volatile and nobody had noticed.** Stderr only, dying with the
  process, after runs that take ten minutes. It contradicted a principle the project
  states explicitly, and it survived because every individual run *looked* fine.
- **Three of four modules recorded nothing**, because a faulting guest died before the
  trace was written. The fault path was the one that most needed it.
- **The worker had no paths configured at all**, so nothing would have been persisted
  anywhere regardless of which path wrote it. It is the only process that ever sees a
  trace.
- **Databases were written with an empty `suffix_hex`** whenever the shipped default was
  used, which makes them unloadable. Every trace printed hashes for names we had already
  found, and it read as the search having failed rather than the file being unusable.
- **Documentation drifted in both directions at once**: a verb that existed and was not
  in the help, and a command in the help that did not exist.

### Outstanding

`sceKernelDirectMemoryQuery` is 99.9% of all guest calls across four executables.
Implementing it is the next piece of work, and it is the first time the project has had
a next piece of work chosen by measurement rather than by roadmap order.

