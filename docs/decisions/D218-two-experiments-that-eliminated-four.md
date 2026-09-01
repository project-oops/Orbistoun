# D218 - Two experiments that eliminated four things and confirmed nothing


**decided** · 2026-08-24 · directed by the user, continuing the wall work from D217

Two mechanisms built, one wall swept with each, and the honest result is that **both
leading hypotheses are now dead**. Recorded because a negative result nobody writes down
gets re-run.

### The map shape, and an experiment that was underpowered

PPSA04263 spends 99.9% of every call walking the memory map and rejecting it - 852 million
queries in twenty seconds, never reaching `sceKernelAllocateDirectMemory` at all. Three
answers had been swept and changed nothing: the return code, the third structure field, and
whether the buffer is cleared. **The map itself had never been anything but one free region
starting at zero.**

That is worse than an untried option. A guest hunting for a region matching some criterion
cannot distinguish "wrong value" from "wrong shape" when there is one region to look at, so
the third-field sweep proved less than it was read as proving - the same shape as D187,
where an `ok` sweep reported no change because the functions under test never saw it.

`Settings::map_shape` (`whole`, `reserved-low`, `fragmented`) is data in the run
configuration, not a rebuild, and defaults to what every earlier measurement was taken
against so changing it is deliberate.

**Result: the guest walks any map correctly and rejects it anyway.** Given four regions -
taken `[0,512M)`, free `[512M,2.5G)`, taken `[2.5G,3.5G)`, free `[3.5G,8G)` - it queried
`0`, `0x20000000`, `0xA0000000`, `0xE0000000`, `0x200000000`, then restarted at zero. Every
region, in order, correct. Map shape is eliminated.

**And one thing it cannot settle, which I nearly claimed it did.** A contiguous map cannot
distinguish `end` from `start + size` in the second field, because each region begins where
the last ended - so a guest feeding back the previous end produces identical offsets under
either layout, whatever the shape. Settling that needs a map with a *gap* in it, not one
with more regions.

### Planting a value, because a stub can only change what a function answers

A stub policy sets a return value. Nothing could set a *side effect* - and both walls had
been narrowed to "an out-parameter nobody wrote" (D217). The only mechanism for a side
effect is a `guest_module!` declaration keyed by a name, and the function on the biggest
wall has none that any source reaches (D213).

`ORBISTOUN_WRITE=<import>:<slot>:<value>` plants a `u64` at the address in an argument
before the import answers. It resolves an import the way forced dumps do, so an **unnamed**
function can be named by its hash. Diagnostic standing, like the stack poison (D185): in
the environment because a question is asked once rather than configured.

Three things it does that matter more than the write itself:

- **Only the stack is writable.** Not the readable list, which includes the image - whose
  runs are protected after relocation, so planting into one would fault inside the emulator
  and produce a crash unrelated to the guest.
- **A request matching no import says so** and plants nothing, rather than reporting a run
  that changed nothing.
- **The counts go in the run conditions**: `1 planted, 0 refused`. A forced write that
  matched an import and then refused every target is indistinguishable from one that landed
  and changed nothing, and the second is a result while the first is a broken experiment.

**Result: `arg0` and `arg5` are not it.** Both point into the stack, both were planted with
`0x11000000`, both landed (`1 planted, 0 refused`), and the fault stayed at `0xfffe0`
exactly. The guest's zero base does not come from either.

### Where that leaves the wall

Eliminated, all by measurement: the stub return (arithmetic on the observed fault address),
unwritten stack (the poison), `arg0`'s target, and `arg5`'s target. `memalign` was ruled out
earlier and is implemented and returning real allocations.

So the guest gets a zero from somewhere this session has not found, and the out-parameter
reading - which was the only surviving explanation an hour ago - is now itself in trouble.
That is a worse position to be in and a better thing to know.

**Declaration by NID was not built.** It was conditional on the out-parameter hypothesis
surviving, and it did not. Building a mechanism to implement unnamed functions in order to
fill an out-parameter nothing has shown to exist would be speculation, which is what
principle 11 refuses.

