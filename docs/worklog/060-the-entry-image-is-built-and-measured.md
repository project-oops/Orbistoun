# The entry image is built, and measured out of the running


Followed D152 through. `orbistoun-loader::process` now builds the System V initial process
stack properly - strings, auxiliary vector, environment, arguments, count, sixteen-byte
aligned at the count. It is the one part of this project with a genuinely published
reference behind it, the target being FreeBSD-derived.

A process cannot be `call`ed, which is worth stating because it is not obvious: the
pushed return address lands exactly where the argument count must sit, and leaves the
stack eight past the alignment the standard requires. So there is a jump-based transfer
that never returns. Nothing is lost by that - the fault handler and the time limit each
persist the call trace from inside the guest's own thread, and those are the paths that
actually fire.

Then the part that matters more than the code: the guest was asked which of it it cares
about. Six runs, two conventions by three arguments (D153).

The answer is mostly no. The argument register is load-bearing and **binary** - zero
faults instantly at `image+0x7a`, any readable pointer gets the full thirty-seven imports
and dies at `image+0xf2f6`. But the real process image and a zeroed block are
indistinguishable, and jump and call are indistinguishable - and those two differ in where
`rsp` points and whether the count survives at all. This entry point is not reading the
stack image at the point it currently reaches.

So the entry image is **eliminated as the wall**, not confirmed. That was the leading
candidate going in. `image+0xf2f6` is still where it dies, and now that is a narrower
question than it was this morning.

The image stays: it is correct, tested, costs nothing, and the `zero` row proves the
mechanism is load-bearing even if not yet in the way it was built for.

The config file the `paths` command has printed since it existed is now actually read. A
malformed one **fails the run** rather than falling back - the failure it prevents is a
typo'd setting silently reverting, the run behaving exactly as before, and "that setting
has no effect" being recorded as a measurement. For a unit whose entire output is a table
of settings against outcomes, that would have poisoned the result.

