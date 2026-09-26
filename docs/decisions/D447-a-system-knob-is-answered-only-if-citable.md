# D447 - A system knob is answered only from a source this project can cite

**Status:** decided
**Date:** 2026-09-01

`sysctlbyname` answers a fixed set of named knobs it has a citable value for, and refuses every
other name.

**Why:** A refused system knob is what turns off a guest's own workaround for it; answering an
invented value instead would be read by a guest as a real fact about the platform. The target
kernel's own family name is a citable constant; a configured release string comes from the active
machine profile; nothing else here has a source.

**Rejected:** inventing a plausible value for an unmeasured knob so a guest's query succeeds -
exactly the fabricated fact this project's honesty rule forbids.
