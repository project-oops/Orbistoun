# D690 - A guest's image descriptor maps to the one bound texture, and a second one is refused

**assumed** - 2026-09-15

`image_sample_lz` names its texture in **eight consecutive scalar registers** and its sampler in
four more. The decoder reads both, because the fields say where each group starts (worklog 549).
What they hold is a descriptor: a base address, an extent, a format, a tiling mode - written into
those registers by the shader itself, usually with an `s_load_dwordx8` from a pointer the command
stream handed it.

The host half of the image subsystem now exists: an image, a view, a sampler and a descriptor at
set 0 binding 2, with two oracle modules reading a known texel through it (worklog 566). The
question this settles is what a guest's eight registers should become on that side.

## The decision

**Every `image_sample_lz` in a module reads the one sampled image the pipeline bound**, and the
translation refuses a module where that cannot be the whole story.

Concretely, three rules:

1. The first sample records the register numbers its descriptor and sampler operands name.
2. A later sample naming a **different** pair is refused, with a message saying the module uses
   more than one texture and only one is bound.
3. A write to **any register inside either recorded range** invalidates the recording, and the
   next sample is refused. Without this the check is worth much less than it looks: a shader that
   loads a second descriptor into the same eight registers and samples again would pass rule 2
   while reading two different textures.

Rule 3 is the one that makes the other two an argument rather than a hope, and it costs nothing -
the translator already sees every write to a scalar register.

## Why refusing is the point

A translation that mapped every sample onto whatever texture happened to be bound would render
**a frame that looks right and is not**. That is the exact wording D104 used to refuse inventing
a colour attachment for an export target other than `mrt0`, and the reasoning transfers without
change: a wrong texture is a plausible picture, and a plausible picture is the failure this
project spends most of its effort avoiding. A refusal that names the instruction and the reason
is worth more than a frame nobody can check.

What a refusal costs here is small. The GL cube - the shader this exists for - binds one texture.
The case that exists is the case that works, and the case that does not is the case that stops.

## Why not resolve the descriptor properly

Because there is nothing to resolve it to. The eight registers describe a surface at a guest
address, in a tiled layout, in a format the descriptor names - and no host image exists behind
any of that. Reading the descriptor would mean constant-folding the scalar loads that filled it,
decoding the fields, and then finding that the answer points at guest memory this backend has
never uploaded.

That is a subsystem - a descriptor decoder, a surface cache, a format table, an upload path - and
it is the right eventual answer. It is not a prerequisite for translating one instruction, and
building it before anything sampled would be building it blind.

## Why register identity rather than a count

A count of distinct textures would need the descriptors decoded, which is the thing that does not
exist. Register identity is the strongest claim available without one, and rules 2 and 3 together
make it a real claim: two textures live in two register ranges, or in one range that is written
between the two samples, and both are refused.

It is still an approximation. A shader that samples the same texture through two different
register ranges is refused although it need not be - a false refusal, which is the safe
direction and the one this project chooses everywhere else.

## What this is not

It is not a claim that a translated shader reads the *right* texture. It reads the bound one, and
which texture that is remains the caller's. This decision is about making a module that needs
more than one say so, out loud, at translation time.

It supersedes nothing. It replaces the `BLOCKED` entry for `image_sample_lz`, whose stated
obstacle - "a descriptor this translator has no model for and a host image view nothing here
declares" - is now half true: the host view exists and the descriptor model still does not.
