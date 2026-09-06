# D553 - The translator draws

**decided** - 2026-09-04

Phase 6 step (c) - emit `Fragment` modules with output variables - and with it the export.
`exp` is out of `BLOCKED`, which is now empty.

## The first end-to-end check this project has been able to make

Phase 6's own page states the cost of building ahead of the spine: **everything in it is
verified against material this project generated, so it cannot be wrong in any way its own tests
would notice.** The framebuffer oracle was built to break that (D549, D550), and this is the
first thing to go through it.

`tests/translated_export.rs` draws twice over the same red clear - once with the hand-assembled
`constant_colour_fragment_module`, once with a module the translator produced from guest
instruction words - and compares **every pixel of the two frames**. Neither side checks itself.

It is a differential, which is the shape every later comparison in this phase takes. The frames
are compared *and* the expected bytes asserted, so a failure says which of the two moved.

## What the fragment stage is

Three differences from a compute module and nothing else: the execution model,
`OriginUpperLeft`, and **whether the epilogue writes the observation window**.

That last one is not cosmetic. A fragment shader may write a storage buffer only where
`fragmentStoresAndAtomics` is enabled, and this project's device requests no features at all
(D552). Skipping the epilogue keeps a fragment module inside what the device can run - and it
also removes the only static use of the observation buffer, so the pipeline needs no descriptor
for it. It costs nothing, because a fragment module's oracle is the attachment.

## What the export translates, and what it refuses

Four sources read for lane zero, **bitcast** rather than converted - a vector register holds the
colour's bit pattern already, and an arithmetic conversion would scale every channel - assembled
into a `vec4` and stored to the output.

Three refusals, each separate and each for its own reason:

- **A module with no colour output.** That is what `Model::colour_output` defaulting to `None`
  is for: a compute dispatch has nowhere to export to, and the failure that matters is not an
  error but a store somewhere arbitrary, which renders a frame that looks plausible and is not.
- **Any target but `mrt0`.** Which attachment another index selects is register state a guest
  writes, needs a capture, and D104 refuses to invent. Translating them all onto one attachment
  would appear to work.
- **The compressed and done flags and the write mask.** They live in the instruction's first
  word, are not among the operands the decoder solved, and are not read - so nothing here may
  claim to honour them.

`orbistoun-spirv` gained `OpCompositeConstruct` and its shape row; `OpConstantComposite` builds
an aggregate from constants and an export's components are values.

## Three breaks, and the one that did not fire is the useful one

- Assembling the components in **reverse** fails the frame comparison - `[255,255,0,0]` against
  `[0,0,255,255]`, every pixel.
- Accepting a **different target** than the shader exports to fails at translation, with the
  refusal quoted.
- Reading **lane one instead of lane zero** *passes*.

The third is not a gap in the break, it is a fact about the shader: its registers are set by
literal moves, and a literal move writes every active lane, so every lane holds the same value
and the choice is unobservable. Distinguishing them needs a shader whose lanes differ, which
needs an input that varies across a fragment - interpolation, which is not built.

That is written into the test rather than left out. A break that does not fire is evidence about
the test's reach, and reporting only the two that worked would have overstated it.

## What this does not do

**It does not make a real shader translate.** The subject is four moves and an export. No
interpolation, no fragment inputs, no blending, no depth, one attachment, one target. `exp` was
listed as the instruction blocking more shaders than any other, and it is now unblocked for the
narrowest possible case - which is the case the oracle can check, and the only one that could
have been checked first.

`BLOCKED` is empty and is kept. The distinction it draws - between *nobody has looked at this*
and *this is waiting on a subsystem* - is what stops a worklist sending effort at whichever
refusal is most frequent, and the next instruction that needs it will need it for the same
reason.
