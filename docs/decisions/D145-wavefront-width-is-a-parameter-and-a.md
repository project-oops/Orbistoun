# D145 - Wavefront width is a parameter, and a narrow shader is a different instruction stream


**Status:** decided (executed on hardware)

D141 established that this generation runs shaders at either width, chosen per shader when
it is compiled, and that the *encodings are identical either way* - so the tables needed no
change. What it did not say is what the translator needs, which turned out to be more than
a lane count.

**A 32-lane shader manipulates its masks with the 32-bit scalar instructions.** Its mask is
thirty-two bits and fits in one register, so it writes `s_mov_b32 exec_lo, 0` where a
64-lane shader writes `s_mov_b64 exec, 0`, and narrows with `s_and_b32 exec_lo, exec_lo, sN`.
Those were being translated as ordinary scalar writes into the register file - so the mask
never changed, every lane stayed active for the whole shader, and the result was a shader
that runs and is not the one the guest wrote. That is the failure mode this project cares
most about, and it was one line from happening silently.

Masks are now written through the mask whenever the destination names one, at either
width, and a mask read as a 32-bit *source* returns its low half instead of being refused
for not being an inline float.

**The width is supplied, not inferred.** `Strategy::Predicated` carries a `Width`, defaulting
to 64. Nothing in the instruction stream states the width, and inferring it from which mask
instructions appear would mean guessing from an absence for any shader whose masks are
untouched at the point of the guess. On a real target it comes from the pipeline state the
guest set up; here it comes from the caller.

**Not derived from a capture.** No capture contains a narrow shader yet, so these are built
the way the reference says a compiler would build them. What the tests verify is that the
translator does the right thing *given* such a shader - not that this is what one looks
like in the wild. Stated here rather than left to be discovered.

