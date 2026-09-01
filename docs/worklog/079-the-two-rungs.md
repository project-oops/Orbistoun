# The two rungs


**Address provenance** (D149): an address that resolves is now counted as evidence about
the GPU-address assumption rather than discarded as a precondition met, and counted apart
from whether the shader translated. The failure path had to be split to do it - "nothing is
mapped there" and "the shader is not translatable" are evidence about different things.

**The submission entry point** (D200): `submit_at` takes an address and a length, which is
the shape a real call site has.

643 workspace tests green.

### Surprises

- **Rung one as stated could not be built.** "Submissions contribute to a run's progress
  block" assumes submissions; nothing anywhere calls `Pipeline::submit`, so a progress line
  would have said the same thing on every run forever. What could be built is the *seam*:
  the entry point a shim will call, tested, with the address measured on the way in. The
  shim itself sits on the loader side - thread 1 already identified the import that marks a
  submission - so this is the half that can exist before the two meet.
- **The measurement was already being taken and thrown away.** Every submission tested the
  address assumption and reported only pass/fail of the shader behind it. Nothing new had
  to be computed; it had to be *kept*.

