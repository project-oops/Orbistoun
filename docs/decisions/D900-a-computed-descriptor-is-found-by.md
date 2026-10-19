# D900 - A computed descriptor is found by running the dispatch's waves

**Status:** decided
**Date:** 2026-10-19

A compute dispatch's buffer access can take a descriptor or scalar offset that the program computes
from lane data. A waterfall loop does this: `v_readfirstlane` of a lane's value, then a load from a
descriptor table at that offset. The D753 prefix run, over user data alone, cannot know such a
value. Instead, the host runs each wave of the dispatch through the program, as it does to find
global loads (D883), and records the access's registers each time the access runs. A waterfall
runs the access once for each distinct value across its wave's lanes, so the run yields every
descriptor and offset the dispatch uses, and no others.

The run goes past the access, through the loop the access lies in, to the loop's closing backward
branch; the program is read from guest memory at the dispatch's address. Every instruction is run.
A `buffer_load_dword*` is evaluated in each running lane through its descriptor: the base, the
scalar offset, the instruction offset, and the lane's vector offset (`offen`) or index times stride
(`idxen`), with zero past the records, as the hardware reads there. The step bound is raised so
each pass of a 64-lane waterfall fits.

Where the access still cannot be bound, its refusal names how many distinct descriptors the waves
reach it with and where their runs ended, or why the waves could not be run.

**Why:** Grand Theft Auto V's compute program at `0x74005b87ce00` picks each lane's store buffer
from a 65,536-entry descriptor table through a waterfall. The 15,517 buffers the table names span
the guest's whole address space, so no one range can be bound for them. The descriptors a dispatch
actually reaches are the ones to bind.

**Rejected:**
- Binding every buffer the table names: tens of thousands of descriptors over all of memory.
- Importing guest memory into the device and addressing it by pointer: a reversal of D747 that
  rests on extensions not every device has.
