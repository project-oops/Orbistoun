# D146 - The subgroup level is the per-lane model with a ballot, not a third model


**Status:** decided (executed on hardware)

`Fidelity::Subgroup` was a stub with a paragraph describing what it would do. It runs now.

**It is not a new model.** The per-lane model already has one invocation per lane, its own
register file and no mask; the subgroup level is that model *plus a mask*. Writing a third
model would have duplicated five hundred lines whose only difference is whether lanes can
be turned off, so `Predicated` carries an `Option<Mask>` and the two levels are the same
code with and without it. `Fidelity::Lane` still refuses every mask it is asked about,
which was the property worth preserving.

**What a subgroup adds is the ability to ask all the lanes at once.** Each invocation keeps
one boolean saying whether its lane is active; `OpGroupNonUniformBallot` turns that into
the mask *word* the guest's scalar instructions expect to read, and writing a mask back is
the reverse - each invocation keeps the bit that is its own. Three instructions, and the
gap between "one invocation is one lane" and "the guest has a 64-bit mask register" closes.

**The width requirement is reported, not assumed.** One invocation is one lane, so the
host's subgroup must be exactly as wide as the guest's wavefront. That is a property of the
device, so `Translated::required_subgroup` states it and the caller checks it against the
device report. Inventing a default would produce a module that is silently wrong on half of
all devices.

**This machine has a 32-wide subgroup**, so the level runs 32-lane shaders today - which is
why D145 came first. On a 64-lane guest it reports that it needs a 64-wide subgroup and the
caller declines rather than running something wrong. Mapping several guest lanes onto one
invocation is the obvious extension and is not built; nothing needs it yet, and the
reporting means nothing silently gets it wrong in the meantime.

**Not derived from a capture.** Like D145, this is built to the reference. What is verified
is that a masked shader behaves - a cleared mask suppresses the write that follows, on a
real device - not that this is what a title's shaders look like.

