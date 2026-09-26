# D675 - Sysctl knobs split between the platform and the profile

**Status:** decided
**Date:** 2026-09-10

A knob whose value is published for the architecture answers on every machine; a knob that
carries one hardware unit's firmware values comes verbatim from the machine profile; live state,
and knobs the hardware itself refuses, stay refused. A profile knob on a machine that carries no
value refuses rather than answering empty.

**Why:** a published constant is the same on every machine, while a kernel banner or a model
string belongs to one unit, and composing the banner from the firmware version would invent its
revision and build date. Available-page counts are one moment of a memory model that is not
orbistoun's. An empty answer on an unset machine would score knobs it knows nothing about as
answered.

**Rejected:**
- Answering empty for every unset string knob: claims a value exists where nothing is known.
- Transcribing live values such as available pages: a constant posing as a measurement of this
  run.
- Composing the banner from the firmware version: invents the revision and the date.
