# 2026-09-04 - (/loop) The fragment path does not need the feature

```
fragmentStoresAndAtomics: offered by the RTX 5070 Ti, NOT enabled - the device asks for nothing
the register file is already Private, so a fragment variant needs no feature at all
suites 135   clippy/fmt/identity clean on both repos
```

Thirty-eighth cron tick. No guest binary (checked, one command). Step (c) was expected to run
into `fragmentStoresAndAtomics`. Checking before designing around it found two things, and both
change the shape of the work.

## The feature is offered and is not enabled

The hardware reports it as available. Orbistoun's device is created with **no features requested
at all**. A Vulkan feature is off until asked for, so a module built on the hardware's answer is
invalid at pipeline creation on the device that actually exists.

`Properties::fragment_stores` now reports it - written as a comparison against the *requested*
feature set, so enabling it later updates the report by construction.
`the_capability_report_describes_what_was_enabled` gates it, catches **over**-reporting only
(the dangerous direction), and prints whether this run proved anything - on a GPU lacking the
feature the two answers agree for an uninteresting reason. Here it is the interesting one.
Broken by reporting the physical device's capability instead of the enabled set.

## And the path does not need it

The premise was that "the model keeps registers and guest memory in storage buffers". **Half of
that is wrong.** The register file - vectors, scalars, program counter, condition code - is
already `Private`. A translated module binds exactly two storage buffers and neither is the
registers: the **observation window** the epilogue copies registers into, and **guest memory**.

The observation window is the *compute* harness's oracle; a fragment module's oracle is the
attachment. Guest memory stays, and reading needs no feature - only a **store** does.

So the fragment variant is: omit the observation window, keep guest memory readable, refuse a
shader that stores to it. The refusal is principle 3, not a limitation - emitting a module the
device cannot run fails at pipeline creation if the driver is strict and is undefined if not.

Materially smaller than "needs a device feature", and derived from what the translator already
does.

## What is still to do

`Wavefront::new` hard-codes `GL_COMPUTE` and `LOCAL_SIZE 1,1,1`; `finish` writes the observation
window unconditionally. The fragment variant needs the execution model, `OriginUpperLeft`, an
output decorated `Location 0` in the entry point's interface, and the export translated into a
store - which needs `OpCompositeConstruct` (not yet in `orbistoun-spirv`) and a bitcast per
component, since a vector register holds the colour's bit pattern.

No capture needed, and all of it checkable by the oracle.

Roadmap step (c) re-sized (check 13).

Decision: [D552](../decisions/D552-the-fragment-path-does-not-need-the-feature.md).
