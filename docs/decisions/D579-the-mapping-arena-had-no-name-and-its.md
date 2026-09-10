# D579 - The mapping arena had no name and its bytes were unreadable

**Status:** measured
**Date:** 2026-09-07

## The blind spot, and what it was hiding

PPSA03416's wall is the platform's asynchronous file path: the guest builds a command buffer
describing the reads it wants and hands it to `sceKernelAprSubmitCommandBufferAndGetResult`,
which nothing here serves (worklog 426). The buffer is the whole question - what it holds is
what an implementation has to understand - and the report said this about it:

```text
arg0 = 0x7400008a3520 -> no region this run mapped, and address-shaped
```

**Two separate gaps, with one visible symptom.**

`orbistoun-thunk` reads a pointee only from a span published as readable, which is the safety
precondition rather than a preference: an argument that is not a pointer is usually a small
integer, and dereferencing it would fault *inside the emulator*. The spans are published before
the guest is entered - the image and the main stack - and **a mapping the guest makes at
runtime is not among them**, so no bytes were ever dumped for one.

And `report::locate` names an address against five region slots. Slot 3 was called `other` and
**nothing had ever registered it**, so even a readable arena address would have printed as a
bare number.

This is D387 again, one region over. Thread stacks had exactly this shape - published before
entry, so every argument a threaded guest passed read as a wild pointer - and the fix there was
to publish the span when it comes into existence. Guest mappings are the other half: they are
where an allocator puts the structures a call is handed.

## Published where a mapping is placed, not at each of the three sites

Three paths place guest mappings - direct memory, `mmap`, and a reserved virtual range - and
each ended in a bare `fill_mapping`. Adding a second obligation as three more call sites is a
hazard this project has now paid for twice: a reporter wired into the fault path and not the
clean-exit path worked for a title that crashed and not for one that stopped (worklog 425).

So `mapping_placed` is the one function that means *a mapping now exists*, and `fill_mapping`
is one of the things it does. The fourth mapping path has one thing to call rather than a list
to remember.

**Readable mappings only.** `protection_from_guest` answers `read: false` for a guest that asks
for write-only or execute-only memory, and publishing that would let a dump read a page the
host refuses - turning a diagnostic into a fault with no relation to the guest.

## The arena is named, and it is the arena rather than the mappings

`Region::Mappings` takes the dead slot, named `guest mappings`.

**A guest that names its own address is not counted.** A hint is honoured wherever it points
(D459), and folding `0x5000_0000_0000` into the extent would stretch the arena across nineteen
terabytes nothing placed anything in, so every stray pointer between the two would be named as
a guest mapping. `arena_end_of` refuses anything below `MAPPING_BASE`, and the test asserts
that refusal rather than the acceptance beside it.

What is registered is the **arena** - where guest mappings go - not the set of mappings. It
spans the gaps between them and the padding unit `next_mapping_base` leaves, so an address
inside it was not necessarily mapped. That is what the name says, and a fault report states
separately whether the address was mapped, so the two together do not overstate. It is the
concession D489 already made for the title's modules, for the same reason and at the same
granularity.

**`None` rather than a zero-length span for an arena nothing has used.** A zero length is
already how the reporter spells an unused slot, so returning one would register the region and
leave every address in it unnamed anyway, with nothing saying which of the two had happened.
Watched failing: with the guard written `end < MAPPING_BASE` the test gets `Some((base, 0))`.

## Registered late, and that limit is stated rather than discovered

Every other region is known before the guest is entered. This one does not exist then and grows
as the guest maps, so it is read in `collected_dumps` - the one place its name is used, and one
that runs after the guest has stopped.

**That is enough for an argument dump and not for the fault handler.** A run killed from
outside names a faulting address in the arena as a bare number. Said here because a region that
is *sometimes* registered is exactly the kind of thing a reader would assume was always.

Here rather than on each of the ways a run can end, for the reason above: that list is what a
reporter keeps being wired into incompletely. Which this change then found a third instance
of - `guest_stopped` called two of the three reporters by hand and so was the site that missed
`paths_opened` when it was added. It calls `what_the_guest_asked_for` now, which is the
function that exists to stop exactly this.

## What it showed on the first run

```text
arg0 = 0x7400008a3520 -> guest mappings+0x8a3520 =
  00 00 01 00  14 00 00 00  01 00 00 00  00 00 10 00
  00 00 10 09  00 74 00 00  00 00 00 00  00 00 00 00
```

A legible structure where there had been a bare number, and it carries a pointer back into the
arena at `+0x10`. **The reading of those fields is not part of this entry** - the change is that
they can be read at all, and inventing an interpretation here is what principle 3 forbids one
level up.

The wall did not move, which is the correct outcome: this observes and changes nothing the
guest can see. 193 imports, the same fault, `files 1 reads, 0 KiB`.

## What this does not establish

**That every heap-shaped pointer is now dumped.** Nineteen values in the same run still report
`no region this run mapped` - host addresses the guest was handed, and mappings made by paths
that do not go through `mapping_placed` if any remain. The count went from all of them to some
of them, and the remainder has not been enumerated.

**Nor anything about the arena at fault time**, per the limit above.

**Nor that the arena's extent is what the guest can read.** It is where mappings were placed;
`sceKernelVirtualQuery` and `region_containing` remain the authorities on whether a particular
address is mapped, and this adds no claim beside them.
