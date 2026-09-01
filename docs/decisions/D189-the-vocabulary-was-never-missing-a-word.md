# D189 - The vocabulary was never missing a word; it was missing a shape


**decided** · 2026-08-21

Two titles named four functions themselves once `printf` could carry the message (D187):
`scePthreadMutexattrInit`, `Settype`, `Setprotocol`, `Destroy`. Confirmed by hash, so they
were *true* - and the generator still could not produce them, which meant the provenance
audit could not account for them (D119). A name this repository cannot derive is exactly
the name that needs explaining.

D168 recorded the gap as a harvest problem: a missing syscall family in the word list.

**That diagnosis is correct, and it is not the explanation for this family.** The harvest
really is incomplete - it read only `lib/libc`, `lib/libthr`, `lib/msun` and `lib/libutil`,
so `socket`, `setsockopt`, `sched_yield` and their neighbours are genuinely absent, and 218
names in the committed database carry records claiming a list that does not contain them.
The audit refuses those, correctly.

But it was the wrong explanation for *this* family, and applying it here cost weeks. Every
token `scePthreadMutexattrSettype` needs was already harvested. One true diagnosis was
reached for and it happened to fit the shape of the question without being its answer -
which is a failure mode worth naming, because it is nearly invisible: a *correct* known
problem is the most convincing wrong explanation available.

### What was actually missing

Every token needed was already in the harvested list. `mutexattr` appears in fourteen names;
`settype` and `setprotocol` appear in `pthread_mutexattr_settype` and
`pthread_mutexattr_setprotocol` exactly. The words were all there.

The vocabulary was built from vendor-*shaped* parts - a module, a verb, an object - and no
combination of those spells `Mutexattr`. **The vendor did not compose that name. It
inherited it from POSIX, whole.** A compositional grammar cannot reach a name that was never
composed, however many words it holds.

So the fix is one shape, not a longer word list: `sce` + the harvested name with each
underscore-separated part capitalised. `pthread_mutexattr_settype` becomes
`scePthreadMutexattrSettype`, which is the exact symbol a real title imports.

### Derived, not written

The `posix` vocabulary is generated from the standard word list at load time rather than
typed into the grammar file, so there is one source and it cannot drift from the list CI
audits. A test regenerates all four names from the harvested list alone - which is what
turns them from *observed* into *derivable*, and closes the provenance question the guest's
own printout had left open.

Cheap, too: one candidate per harvested name, against millions from the compositional
patterns.

### The lesson worth keeping

A search that misses proves only "not among what was tried", and the instinct is always to
try more words. Twice now the answer has been a different *kind* of candidate rather than
more of the same kind. When a family resists naming, ask what shape it is before asking what
words it needs.

