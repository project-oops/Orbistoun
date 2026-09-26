# D436 - An unresolved title wall is escalated to a hardware measurement, never guessed

**Status:** assumed
**Date:** 2026-09-01

When a guest wall traces back to a value this project's own implementation returned, and no
lawful source documents what that value should be, work on it waits for a conformance probe
measurement rather than proceeding on a guess.

**Why:** The walls this applies to share one shape - a guest's own allocator or address-space call
receiving a value from an implemented handler and trusting it unchecked - and the guest's later
behaviour depends on exactly what that value is, which only a hardware measurement settles.

**Rejected:** inferring the value from the guest's later behaviour alone - indistinguishable from
inventing it.
