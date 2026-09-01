# Automated stub-semantics search

The bisection loop (edit policy, relaunch, observe) is mechanical and currently
manual. Wrap it: given an unresolved function, try a small ranked set of candidate
semantics derived from its name, its FreeBSD analogue, and observed argument usage,
and report which let the guest proceed furthest.

Worth stating the constraint plainly, because it is the whole design problem: each
query costs a boot and returns one bit. The value of any prior - heuristic,
analogue-derived, or model-assisted - is entirely in reducing the number of queries.
Anything that generates plausible implementations without a verification step makes
the codebase worse, not better; see [TESTING.md](../TESTING.md).

Validate the harness against a target with total ground truth (an instruction test
suite) before pointing it at anything unverifiable.

