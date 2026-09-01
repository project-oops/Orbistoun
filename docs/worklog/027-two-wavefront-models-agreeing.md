# 2026-08-20 - Two wavefront models, agreeing


**Done.** Stubs for all three fidelity levels, then the second one built.

- `Fidelity` as a field of `Strategy::Predicated`, so invalid pairings are
  unrepresentable rather than rejected.
- `wavefront.rs`: registers as arrays indexed by lane, the mask in the scalar file
  where the hardware puts it, masked writes by select rather than branch.
- A differential test running four programs at both levels and comparing.

**Verified.** Gate green, device tests executed. Nine execution tests pass on hardware,
including **the two models agreeing on every program tried**.

**Surprises.**

- **The first attempt at splitting a long function saved one line.** `wavefront::new`
  tripped `too_many_lines`; passing nine identifiers to helpers in a struct moved the
  length from the body to the call site and achieved 119 to 118. The identifiers were
  the bulk, so the helpers now allocate their own and return only what the caller uses.
  Worth remembering: when a split does not shrink anything, the split is wrong rather
  than the limit.

- **Implementing a level broke the test asserting it did not exist**, exactly as
  intended, and it is worth having felt: a loudly-stubbed path is pinned by a test, so
  building it fails the suite until somebody consciously removes the pin. That is the
  cheapest possible reminder to update the docs claiming it is a stub.

- **Every lane must be switched on at entry.** The mask is null-initialised, so left
  alone it is zero - every masked write would be discarded and the shader would produce
  a plausible buffer of zeros while executing nothing whatsoever. There is a test for it
  now, because that failure looks exactly like a shader that ran correctly and did
  nothing.

**Not done.** `Subgroup` is still a stub. The two built levels agree on four programs
using four instructions, which is a start and not a proof. And the duplication between
`predicated.rs` and `wavefront.rs` is real - deliberately deferred until two
implementations existed, and now they do, so the seam is observable and the factoring
is owed.

