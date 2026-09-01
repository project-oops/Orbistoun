# D277 - A data breakpoint fires after the access, and pretending otherwise would be a lie


**decided** · 2026-08-25 · a limit of the hardware, stated rather than hidden

An x86 data breakpoint is a **trap**, not a fault: the access completes, and only then is
the exception raised. So the instruction pointer in the report is the instruction *after*
the one that made the access, not the one that made it.

Naming the instruction that actually did it means knowing how long it was, and instruction
length comes from decoding it - which is disassembly of a vendor binary, and therefore
refused by principle 1. That is not a gap to be closed later by a cleverer implementation;
it is the boundary this project draws, meeting a property of the hardware.

So the report says `after the access at ...` and never `at`. The distinction is small and
it is exactly the sort of small distinction that becomes a wrong conclusion three runs
later: the same wall was already misread once by treating a mapping that moved it as
confirming the hypothesis behind the mapping (D224, D226). One instruction of imprecision
that is stated costs nothing; one that is not stated costs a diagnosis.

The offset is still bounded - x86-64 instructions are at most fifteen bytes - so
`locate` puts the access within one instruction of a named region offset, which is what the
question needs.

