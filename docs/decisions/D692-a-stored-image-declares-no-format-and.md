# D692 - A stored image declares no format, and the device feature is what makes that possible

**assumed** - 2026-09-15

A guest's `image_store` writes a texel to an image. On the host that is a **storage image**, and
a storage image in SPIR-V normally names its format in the module: `Rgba8`, `R32f`, and so on.
The format decides how the bits a shader writes are interpreted on the way in.

The guest's format is in the image descriptor - eight scalar registers this project does not
decode, and D690 is the decision not to. So there is nothing to read the format out of.

## The decision

**The module declares `Unknown` and the capability that permits it**:
`StorageImageWriteWithoutFormat`, with the matching device feature
`shaderStorageImageWriteWithoutFormat` requested where the device offers it. The format is then
the pipeline's business - whatever image the host bound - rather than something the shader
asserts.

A device that does not offer the feature cannot run a module that stores to an image. That is
reported before it is asked for, so a caller sees a skip naming the reason rather than a
pipeline failing to create.

## Why not name a format

Because there is only one way to get one, and it is to invent it.

Picking `Rgba8` would work on every texture that happens to be eight bits a channel and would
**silently reinterpret every texture that is not**. A shader writing a float would land as a
clamped byte, or a byte would land as the low bits of a float, and the frame would render. That
is the failure this project is least able to detect and the one D104 and D690 both already refuse
in their own areas: a frame that looks right and is not.

The alternative that avoids inventing anything is to decode the descriptor, and that is the
subsystem D690 declined to build. This decision does not change that; it makes the store possible
without it.

## Why not refuse the instruction instead

Refusing was the honest option while nothing could be done, and it is what the instruction got
until now. It stopped being the right answer once the format question had an answer that needs no
guess: `Unknown` is not a weaker claim than `Rgba8`, it is **a different kind of claim** - it says
the shader does not know, which is true, rather than saying it is eight bits, which is not known.

## What this does not settle

**A guest's load and store of the same descriptor are two host objects here.** A sampled image is
read through binding 2 and a storage image is written through binding 3, and nothing makes them
the same image. A shader that stored to a texture and then read it back would, on the host, write
one image and read another.

That is a real gap and it is left open deliberately: closing it means deciding whether every
texture is bound twice, or whether a sampled read becomes a storage read, and neither should be
decided before something needs it. What exists today is enough for a shader that only writes, and
a shader that does both would be wrong in a way nothing here would catch - which is worth the
sentence it takes to say.

## What it costs

One binding, one capability, one device feature, and a report line saying whether the device has
it. The storage image is created for every pipeline, like the texture and the two buffers, for
the reason those are: which bindings a translated module declares is not something the harness
reads, and a binding a module uses and the layout lacks is an invalid pipeline.
