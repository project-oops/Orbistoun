# D171 - Out-parameters are always written

**Status:** decided
**Date:** 2026-08-21

A function that answers through an out-pointer always writes it, with zero where the value is
unknown, and reports success.

**Why:** an unwritten out-pointer leaves whatever the stack held, which differs from run to run
and leaves no signature in a trace. Zero is an ordinary state a title must already handle - the
first index, off, none - while any other value is a guess dressed as a default.

**Rejected:**
- Leaving the out-parameter unwritten: nondeterministic and unrecognisable.
- A plausible non-zero default: invented behaviour.
