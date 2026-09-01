# Float controls, then narrow wavefronts


Two of the three pieces asked for. 592 workspace tests green.

**The division sequence is complete** (D144). The pre-scale needed no float controls in the
end - and could not have had them, because this device does not support preserving 32-bit
subnormals at all.

**Thirty-two-lane shaders translate** (D145), with the width carried on the strategy.

### Surprises

- **The device answered the question by saying no.** The first thing the new device report
  printed was `subnormals flushed`. The plan had been to request preservation; the plan was
  impossible. It turned out not to be needed: zero and subnormal share an all-zero exponent
  field, and everywhere the question is asked the true value cannot be zero - so one test on
  the bits is correct on a flushing device and a preserving one alike.
- **The report also settled the next task's main parameter.** Host subgroup width is 32 and
  the guest wavefront is 64, so a subgroup mapping is 2:1 rather than 1:1 - which is exactly
  what made narrow-wavefront support worth doing before it.
- **A narrow shader is not the same shader with fewer lanes.** It uses different
  instructions for its masks, and those were landing in the register file instead of the
  mask. Every lane would have stayed active for the whole shader.
- **My own test asserted the wrong thing about lanes.** The pre-scale's flag came back as
  all ones rather than 1, because every lane gets the same operands and every lane flags.
  Asserting `1` would have been asserting the other sixty-three lanes did not run.

