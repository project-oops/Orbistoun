# D552 - The fragment path does not need the feature, and the report must not claim it

**decided** - 2026-09-04

Step (c) - emit `Fragment` modules with output variables - was expected to run into
`fragmentStoresAndAtomics`, because a translated module writes storage buffers and a fragment
stage may only do that with the feature enabled. Two things were checked before designing around
it, and both change the shape of the work.

## The feature is offered and is not enabled

The RTX 5070 Ti here reports `fragmentStoresAndAtomics` as available. Orbistoun's device is
created with **no features requested at all** - `DeviceCreateInfo` carried a queue and nothing
else.

A Vulkan feature is off until asked for, so those are different answers to the same question, and
the useful one is the second. A module built on the hardware's answer is invalid at pipeline
creation on the device that actually exists.

`Properties::fragment_stores` now reports it, written as a comparison against the requested
feature set rather than as a literal `false`, so enabling it later updates the report by
construction rather than by somebody remembering.

`the_capability_report_describes_what_was_enabled` gates it, and says what it cannot do: it
catches **over**-reporting only. A report claiming less than was enabled wastes an opportunity
rather than producing an invalid module, and is invisible here. The test also states when it
proves nothing - on a GPU that does not offer the feature the two answers agree for an
uninteresting reason, so it prints which case this run was. On this machine it is the
interesting one.

Broken by reporting the physical device's capability instead of the enabled set, which is the
easier of the two to write and the one that matters.

## And the fragment path does not need it

The reason the feature looked unavoidable was that "the whole model keeps registers and guest
memory in storage buffers". **Half of that is wrong.** The register file - vectors, scalars, the
program counter, the condition code - is already `Private`, initialised and indexed in place. A
translated module binds exactly two storage buffers, and neither is the registers:

- the **observation window**, which the epilogue copies registers into so a test can assert on
  them, and
- **guest memory**, which loads and stores reach.

The observation window is the *compute* harness's oracle. A fragment module's oracle is the
attachment, so it does not need one. Guest memory stays, and reading it needs no feature at all -
only a **store** does.

So the fragment variant is: omit the observation window, keep guest memory readable, and
**refuse** a shader that stores to it. That last part is principle 3 rather than a limitation:
emitting a module the device cannot run would fail at pipeline creation if the driver is strict
and produce undefined behaviour if it is not.

That is a materially smaller change than "needs a device feature", and it is derived from what
the translator already does rather than chosen.

## What is still to do

The stage seam itself: `Wavefront::new` hard-codes `execution::GL_COMPUTE` and
`EXECUTION_MODE LOCAL_SIZE 1,1,1`, and `finish` writes the observation window unconditionally.
A `Fragment` variant needs the execution model, `OriginUpperLeft`, an output variable decorated
`Location 0` and named in the entry point's interface, and the export translated into a store to
it - which needs `OpCompositeConstruct`, an opcode `orbistoun-spirv` does not yet carry, and a
bitcast per component because a vector register holds the colour's bit pattern.

None of it needs a capture, and all of it is checkable by the oracle: translate a shader whose
export is a constant, run it through `draw_with`, compare against the hand-written fragment
shader that already works.

## The note this repeats

The prompt that set this tick said the model "keeps registers and guest memory in storage
buffers - which a fragment stage may write only if the device reports `fragmentStoresAndAtomics`
- **check that before designing around it**". Checking it found the feature disabled *and* the
premise half wrong. Both were a grep away, and the second one halves the work.
