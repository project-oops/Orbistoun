# D580 - A watch that killed the run, and a message that said mapped when it meant published

**Status:** measured
**Date:** 2026-09-07

## The diagnostic could not be pointed at the thing it was for

D579 made a guest's runtime mappings readable, so the command buffer PPSA03416 submits could
be dumped. Thirty-two bytes is the dump's fixed window and the structure is larger, so the next
step was `ORBISTOUN_WATCH`, which takes an address and a length.

It killed the run before the guest started:

```text
orbistoun: presenting a ps5/cex/base machine
reached ContainerParsed … reached Linked
outcome Crashed { access violation }
```

`snapshot` dereferences the address unconditionally. **Its own documentation said it did not** -
*"silent when the region cannot be read: … an address that is not mapped is an ordinary mistake
rather than a reason to end the run"* - directly above a raw `from_raw_parts`, with a SAFETY
comment underneath admitting the opposite (*"an address outside it faults here"*). Three
statements, two of them wrong, in fourteen lines.

That is principle 3 one layer down: **a doc promising what the branch below it never did** is
the same failure as a message naming a cause nothing measured, and D385 has the identical shape
(`RUNTIME_GLOBALS`, where the doc promised `zero` and the code did the other thing).

## Fixed on both sides, because the absent case is the interesting one

A region that does not exist at entry is recorded as absent rather than read. `changes` then
asks again *after* the guest has stopped - a different answer for anything the guest mapped
while it ran - and reports what is there.

**That is not a degraded mode, it is the common one.** A command buffer, a descriptor, anything
an allocator handed the guest is mapped after entry, so the diagnostic that was built to answer
*"which slot did nobody fill in?"* could never be aimed at any of them. It reads them now:

```text
orbistoun: watched region:
  0x7400008a3520+0x100 did not exist when the guest started
  0x7400008a3520  0x0000001400010000
  0x7400008a3528  0x0010000000000001
  0x7400008a3530  0x0000740009200000
  …
```

The readability test moved to `orbistoun_thunk::readable_span`, which is what the argument dump
already used for its own fixed window. **One answer to "may I dereference this?"**, because a
second implementation is a second chance to fault inside the emulator on an address the guest
never touched.

## The message said *mapped* and meant *published*

Both this and the argument dump reported an address they could not read as being in *"no region
this run mapped"*. They had checked no such thing. What they know is that **nothing published
the span to them**, and for months those two came apart across every mapping a guest made at
runtime - so a pointer into an ordinary heap structure was reported in the words used for a
wild pointer.

The dump now says *"in no span this run published as readable, and address-shaped"*, and the
watch says the same. The distinction is not pedantry: it is the difference between *the guest is
wrong* and *the tool cannot see there*, and this session spent its first hour on a wall that
turned out to be the second.

**The fault path is untouched.** `diagnose.rs` reports *"an address in no region this run
mapped"* about a faulting address, which it establishes from the trace's own region list rather
than from the readable spans. Same words, different and correct claim.
## What it then showed, and the reading was wrong

The command buffer's word at `+0x10` is a pointer into the mapping arena. A watch on that
address reported no published span, and this entry read it as *the guest holds a buffer
orbistoun never mapped for it*.

**It is mapped.** Asked from inside the call that receives the pointer, where the two cannot be a
run apart: `0x740009200000..0x740009300000`, one mebibyte, exactly the length the header states.
The address had been typed in from a *previous* run while the arena moves between runs (D582), so
the pairing was never sound - and "no published span" was read as "not mapped", which is the
distinction this same entry renamed a message to make.

The caveat below was correct and was the load-bearing part; it is kept as written because being
right about the limit and wrong about the conclusion anyway is the whole lesson. The two causes
of the non-publication - a write-only mapping recorded as unreadable, and a table of readable
ranges that had silently run out of room - are D588.

**This is not established as the cause of anything.** The tool distinguishes *unmapped* from
*unpublished* no better here than anywhere else - a mapping path that does not go through
`mapping_placed` would look identical - and only one run was checked as a matched pair. It is
recorded because it is the kind of thing that gets assumed away.
recorded because it is the kind of thing that gets assumed away.

## And the address is not the same twice

Four runs of one build, reading the same word:

| run | `+0x10` | reservations failed |
|---|---|---|
| 1 | `0x740009200000` | 2 |
| 2 | `0x740009100000` | - |
| 3 | `0x740009200000` | - |
| 4 | `0x740009200000` | 2 |

**Two runs of one build do not agree**, which D181 and D238 require of every measurement here.
It moves with the reservation conflicts, so the arena's bump allocator is being driven by
something that varies. Not diagnosed, and it is the reason a watch has to be aimed by hand at an
address read out of a previous run - the automation this entry would otherwise have earned.

## What this does not establish

**That the fixed watch reads every structure worth reading.** It reads one span, given an
address a person typed after a previous run printed it. Following a pointer *out of* a dumped
argument is the step that would remove the guessing, and is not attempted here.

**Nor that the remaining unreadable pointers are the guest's fault.** Nineteen in a run still
report the message above; some are host addresses that legitimately are not guest memory, and
the split has not been counted.
