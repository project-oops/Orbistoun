# The third-source idea had no consumer, and I had already written that it did


Asked what to do next given the clarified provenance rule, and checked rather than
answered from the entry written an hour earlier. The entry was wrong.

D206 justified reading LLVM's TableGen files as a third source by naming two things
`BLOCKED` in `model.rs` for want of exactly that kind of fact: the hidden condition-code
side effects and the division thresholds. **Both are implemented** - the condition-code
behaviour since D129, the division sequence once the published reference supplied its
thresholds. The single remaining `BLOCKED` entry is `exp`, and no table settles it: it
needs a render target to export to, which is a decision about this project rather than a
fact about the hardware.

Corrected in both places, and the REFERENCES.md paragraph the claim was copied from is
fixed too - it still described the division sequence as refused.

### Surprises

**The stale claim was one paragraph away from a sweep that was looking for stale claims.**
The documentation pass earlier today grepped for sentences that would be false if recent
changes were right, and found seven. It did not find this one, because the division work
predates the changes that sweep was checking - the search was scoped to what had just
moved, and this had been wrong for longer.

**And I propagated it within the hour.** Writing D206 I reached for a justification, found
one already written down in a file I had just edited, and repeated it without checking. A
stale document is not just wrong in place; it is a source that new writing cites.

The useful correction is to the *method*, not the entry: a justification that names
specific code should be checked against that code as it is written, not treated as
established because it appears in the tree.


