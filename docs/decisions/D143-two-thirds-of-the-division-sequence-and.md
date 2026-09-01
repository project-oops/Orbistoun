# D143 - Two thirds of the division sequence, and a sharper reason for the third


**Status:** decided (executed on hardware)

D124 refused the division sequence because the thresholds and the special-case table were
in a published document this project did not have. It has the document now (D139 fetched
it for the encoding families), so the refusal had to be revisited rather than inherited.

**`v_div_fixup_f32` and `v_div_fmas_f32` are translated.** Both are given in full as
pseudocode, and both are now executed against expected bit patterns on a real device -
five tests covering the indeterminate forms, the signed zeros and infinities, quiet-NaN
propagation, the ordinary quotient, and the mask-conditional scaling.

**Three terms the reference does not define numerically** - `Quiet`, `underflow`,
`overflow` - are read as **IEEE-754** defines them. That is a lawful, citable reference
rather than a guess, and it checks out against the reference's own threshold: the
underflow branch triggers below an exponent difference of -150, which is exactly where a
quotient falls under half the smallest subnormal and rounds to a signed zero under
round-to-nearest. A number arrived at two independent ways is not an assumption.

**`v_div_scale_f32` is still refused, for a different reason.** Most of its decision tree
is a test on the operands' bit patterns and needs nothing. Two branches ask whether
`1/denominator` and `numerator/denominator` are *subnormal*, which cannot be answered
from the inputs alone - and answering it by dividing and inspecting the result only works
if the host preserves subnormals. Vulkan lets an implementation flush them to zero, and
flushing makes both tests answer false, which silently disables exactly the scaling the
instruction exists to perform.

That is a better blocker than the one it replaces. The old one said *we have not read the
document*; the new one says *request subnormal preservation through the float controls and
confirm the device honours it*. A test pins the new wording so the entry cannot quietly
revert.

**`BLOCKED` is keyed by name now**, like `SUPPORTED` before it. It was keyed by family and
opcode number, with numbers from the previous generation, so after the retarget every
entry pointed at whichever instruction occupied that slot - and a carefully written
explanation of why something is blocked would have been offered for something else. A test
now also asserts every blocked name exists on this target, because a reason nobody can
reach is not a worklist entry.

