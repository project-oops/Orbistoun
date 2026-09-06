# 2026-09-04 - (/loop) The translator draws

```
phase 6 step (c) DONE - a translated `exp mrt0` puts its colour on the screen
BLOCKED is now empty; exp is in SUPPORTED
suites 136   clippy/fmt/identity clean on both repos
```

Thirty-ninth cron tick. No guest binary (checked, one command).

## The first end-to-end check this project has been able to make

Phase 6's page states the cost of building ahead of the spine: everything in it is verified
against material this project generated, so it cannot be wrong in a way its own tests would
notice. The framebuffer oracle was built to break that, and this is the first thing through it.

`tests/translated_export.rs` draws twice over the same red clear - once with the hand-assembled
fragment shader, once with a module the **translator** produced from guest instruction words -
and compares **every pixel of the two frames**. Neither side checks itself. A differential, which
is the shape every later comparison takes.

## The fragment stage

Three differences from a compute module: the execution model, `OriginUpperLeft`, and **whether
the epilogue writes the observation window**. The last is not cosmetic - a fragment shader may
write a storage buffer only where `fragmentStoresAndAtomics` is enabled, and this device requests
no features (D552). Skipping the epilogue keeps the module inside what the device runs, and
removes the only static use of the observation buffer so no descriptor is needed.

## The export

Four sources read for lane zero, **bitcast** rather than converted - a register holds the
colour's bit pattern, and a conversion would scale every channel - assembled into a `vec4` and
stored. Three refusals, each separate: a module with no colour output; any target but `mrt0`
(which attachment another selects needs a capture, D104); and the compressed/done flags and write
mask, which are not among the solved operands and so may not be claimed.

`orbistoun-spirv` gained `OpCompositeConstruct` and its shape row.

## Three breaks, and the one that did not fire is the useful one

Reversing the components fails the frame comparison, every pixel. Accepting a different target
fails at translation. **Reading lane one instead of lane zero passes** - because the shader's
registers are set by literal moves and a literal move writes every active lane, so the choice is
unobservable. Distinguishing them needs a shader whose lanes differ, which needs interpolation.

Written into the test. A break that does not fire is evidence about the test's reach, and
reporting only the two that worked would have overstated it.

## What this does not do

It does not make a real shader translate. Four moves and an export; no interpolation, no fragment
inputs, no blending, no depth, one attachment, one target. `exp` is unblocked for the narrowest
case - the one the oracle can check, and the only one that could have been checked first.

Roadmap step (c), G11 and the refused-instruction list rewritten (check 13).

Decision: [D553](../decisions/D553-the-translator-draws.md).
