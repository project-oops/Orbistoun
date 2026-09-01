# D144 - The division pre-scale needs no float controls, and could not have had them


**Status:** decided (measured on hardware)

D143 left `v_div_scale_f32` blocked on what looked like a capability problem: two of its
branches ask whether a computed quotient is *subnormal*, Vulkan lets an implementation
flush subnormals to zero, and the fix appeared to be requesting `SPV_KHR_float_controls`.

**The device does not support it.** The first thing the new device report said was
`subnormals flushed` - `shaderDenormPreserveFloat32` is false on this hardware, which is
ordinary for this vendor. Declaring the capability would not have preserved anything; it
would have made every module unloadable.

**It does not matter.** A flushed subnormal and a preserved one give the *same answer* to
the question being asked, because the question is only ever asked where the true result
cannot be zero - a reciprocal of a finite non-zero value, or a quotient with a non-zero
numerator, both guaranteed by branches that run first. Zero and subnormal share an
all-zero exponent field, so one test on the bits answers it on either kind of device:

- preserving: the exponent is zero because the value is subnormal;
- flushing: the exponent is zero because a subnormal was flushed to zero.

Testing the *bits* rather than doing a comparison is what makes this hold - a comparison
against the smallest normal would itself be at the mercy of how a flushed operand
compares.

So the sequence is translated in full, and it is **more** portable than the float-controls
route would have been: it needs no extension, no capability, and no device support.

**The device report stays anyway.** It is what found this, and the subgroup width it also
carries is what the next piece of work is built on. Asking a device what it can do is
worth doing even when the answer turns out to be "not that".

**What the reference does not say.** The pre-scale's zero-operand branch is written
`D.f = NAN` with no bit pattern, where the fixup gives one explicitly. The canonical quiet
NaN is used and the difference is unobservable: this result feeds the reciprocal and then
the fixup, which replaces any NaN with a quietened operand of its own.

