# D016 - Validation separated from effects

**Status:** decided
**Date:** 2026-08-19

Where it fits, a decision is a pure function and its effect is a thin wrapper that calls it:
`AddressSpace::validate` decides, `reserve` validates and then acts.

**Why:** most effects here - mapping memory, entering guest code - are hard to test. A pure
decision function makes the rules fully testable without touching the host.

**Rejected:**
- Decision and effect in one function: the rules are tested only by running the effect.
