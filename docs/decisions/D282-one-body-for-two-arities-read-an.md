# D282 - One body for two arities read an argument it was never passed


**decided** · 2026-08-26 · the probe crashed the emulator, not the guest

The vendor and POSIX spellings of the same primitive differ in **arity**, not only in name.
`scePthreadRwlockInit` takes three arguments - lock, attributes, **name** - and
`posix_pthread_rwlock_init` takes two. Both were bound to one implementation, and that
implementation read `args[2]` unconditionally.

For the POSIX call, `args[2]` is whatever the guest happened to leave in `rdx`. Here it was
`0x3f`, and `read_name` dereferenced it: `read of 0x3f` at a **host** instruction pointer.
The emulator died, in its own code, on a guest call that was perfectly well formed.

Three things this is worth recording for.

**A crash inside the emulator reads as a guest fault.** The report said *"the guest faulted
at `0x7ff63fa1da58`"*, and the address is ours. Every register in it is ours. Nothing in the
wording distinguishes "the guest did something we do not handle" from "we dereferenced
rubbish", and those want completely different work.

**The arity is declared, and the implementation cannot see it.** `guest_module!` records
`=> 2` and `=> 3` a few lines apart; the function takes `&[u64; GUEST_ARG_REGISTERS]` and has
no idea which of the two called it. The declaration knows the answer and the code that needs
it is not given it - so the separate entry points are the fix, not a runtime check.

**`read_name` trusts a guest pointer with nothing checking it**, which is the larger hole and
is not specific to this call. It scans up to sixty-four bytes from an address the guest chose,
one byte at a time, with a comment observing that this bounds the overshoot rather than
preventing it - and a one-byte overshoot into an unmapped page faults exactly as hard as a
whole one. Any guest-supplied string pointer can end the run, in the emulator, from any
implementation that names something. Principle 4 already says where that belongs: guest
memory access is `orbistoun-mem`'s, behind a checked accessor, and this is a raw
`std::ptr::read` in a subsystem crate.

The immediate fix is separate entry points, because it is unambiguous and removes the crash.
The checked accessor is the one that matters and is recorded as a thread rather than done in
the same breath, because it touches every caller that reads a guest string.

**A static guard for this was attempted and does not exist.** The obvious one - two names
sharing a body must agree on their declared arity - was written, and it failed immediately on
`snprintf_s` (6) and `snprintf` (3), which share a body **correctly**: both pass destination,
size and format in the first three registers. `ImportDesc::arity` is documented as a hint for
how many registers a trace should record, *"wrong arity degrades trace quality; it does not
break the call"*, and it is deliberately inflated for variadics. So disagreement is legitimate
and the guard was checking a different field from the one it claimed - the precise failure
principle 3 names. It was removed rather than kept with an exception list.

What actually caught this was the **conformance probe**, which calls the POSIX spellings
deliberately and by name. That is an argument for running it in the gate: a unit test cannot
easily assert "this body does not read past its arity" without a process boundary per call,
and the probe demonstrates it by doing it.

