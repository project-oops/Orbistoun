# D194 - Argument dumps are bounded and taken at call time

**Status:** decided
**Date:** 2026-08-22

The dispatch path records the first calls to each unimplemented import - or any import named in
`ORBISTOUN_DUMP` - with every argument's value and, for an argument pointing into memory this run
mapped, a short capture of what it points at. Everything is bounded and allocation-free, and the
handler is looked up once per call.

**Why:** for an out-parameter or a descriptor, what the argument points at is the whole of the
information, and a stack frame is reused within microseconds. The mapped-range check is both the
safety precondition and the filter that stops a count being read as an address. Scalars are
evidence in their own right.

**Rejected:**
- Reading at collection time: a confident, wrong answer.
- Dereferencing any argument: a length faults inside the emulator.
- Dumps for unimplemented calls only: a suspected implementation cannot be inspected.
