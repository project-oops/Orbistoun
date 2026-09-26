# D010 - Honest failure over plausible output

**Status:** decided
**Date:** 2026-08-19

Where orbistoun does not know an answer it says so: an empty result is an error, and no
constant, error code or arity is invented to make something compile or run.

**Why:** an empty import list reads as "this title needs nothing", which is never true. A
plausible wrong answer is acted on; an explicit "not handled" costs the same to write and is
worth more.

**Rejected:**
- Returning empty or default values on failure: indistinguishable from success.
- Guessed constants marked for later review: the guess ships as fact.
