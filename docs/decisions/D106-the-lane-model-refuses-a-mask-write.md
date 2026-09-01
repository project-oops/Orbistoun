# D106 - The lane model refuses a mask write; Auto reads the shader to choose, and says so


**Status:** decided (confirmed with input, 2026-08-20)

`Model::write_mask` is a required trait method that returns a `Result`. The wavefront
model writes both halves of the mask as ordinary scalar registers. The per-lane model
returns an error.

Doing nothing was the alternative, and it is the dangerous one: that model has one
invocation per lane and no way to represent an inactive one, so a shader that disables
lanes would run every lane regardless. Plausible output, wrong answer, nothing in it to
indicate the problem - the exact failure D098 keeps three levels to avoid.

**`resolve` now takes the decode.** `Fidelity::Auto` picks the wavefront model for a
shader that touches the mask and the lane model otherwise. It previously answered the
lane model unconditionally, and the function said why: any shader needing more would
contain an instruction the translator refused anyway, so the wrong level could not be
chosen - "not by analysis, but because the translator stops first". The same comment
noted this was safety by accident and would expire when those instructions were built.
It expired in the change that built them.

This is not the silent substitution D098 forbids. `Auto` is a request to be told what the
shader needs; asking for a level explicitly and getting it is unaffected, and a shader
that needs a mask is refused loudly by the model that has none.

**`touches_mask` inspects operands, not opcodes.** `s_mov_b64` needs a mask when its
destination is `exec` and not when it is an ordinary register pair, so a table keyed on
the opcode would have to say yes to both and push every shader containing any 64-bit move
onto the slow model.

### Confirmed, and the fallback is now a warning

The open question was whether `Auto` should silently upgrade to a much slower model or
refuse and make the caller ask. **It upgrades - and it says so.**

Refusing was never really available: a translator that refuses whenever a shader masks
would refuse nearly every real shader, which is not a useful default. But the cost is a
factor of sixty four, and one instruction touching `exec` anywhere is enough to trigger
it, so leaving the only evidence in a field was the wrong side of it. A field is something
a caller has to know to look for; a warning is something a caller has to decide to ignore.

`Translated::warnings` carries it, the submission report collects it, and the message says
what forced the fallback.

**It also carries the width a subgroup would need**, because that is the actionable part.
`Fidelity::Subgroup` (D146) is as fast as the per-lane model *and* has a mask, so it is
the right answer whenever it fits - and whether it fits is a property of the *device*, not
of the shader. The translator has never seen a device, so it reports the requirement and
leaves the comparison to whoever knows. That is also why `Auto` does not simply choose the
subgroup level itself: it cannot, without knowing what it is running on.

A shader that needs no mask warns about nothing, so the warning stays worth reading rather
than becoming noise every caller learns to skip past.

