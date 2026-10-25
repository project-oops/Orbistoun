# D963 - A dispatch binds the storage images it stores to

**Status:** decided
**Date:** 2026-10-25
**known_by:** assumed

A compute module may store to up to sixteen images, each at a binding of its own: the first at
binding three, the rest from 37 on, after the last texture binding. Sixteen is fixed, so every
module's bindings are laid out alike. A dispatch storing to more images than its device lets one
stage reach (`maxPerStageDescriptorStorageImages`) is refused by name, and so is a module storing to
more than sixteen.

A store names the image the load reaching its descriptor registers read, as a sample does (D690),
whatever registers it goes through. Registers still holding the user data they were seeded with
name that word's image. Otherwise a store names the image last stored through the same registers,
unless something no load or user-data word names rewrote them since. In that case they hold another
image, found for each dispatch where the store is, as a computed fetched image is (D960).

**Why:** Grand Theft Auto V's full-screen compute pass at `0x74005b809c00` stores to six images,
reloading `s[16:23]` from one table offset after another. Matched by register, the store at `0x84c`
through `s[16:23]` reloaded from `0xa0` reused the image loaded from `0x120`, and wrote the wrong
one. With the store matched by load, the six were more than D893's four bindings. Four is the least a
Vulkan device lets one stage reach, but the desktop devices orbistoun runs on allow far more. The
storage bindings at 21 to 23 also shared their numbers with texture slots 16 to 18, which D690's
thirty-two textures reach. A device test stores one image to six, each reloaded into `s[16:23]`, and
all six come back equal to the source.

**Rejected:**
- Sizing the bindings to each device's limit: a module's layout would depend on the device it was
  translated for, and kept translations and pipelines are shared across runs.
- Keeping four and refusing past it: the program's six images are named exactly, so nothing has to
  be guessed.
- Matching stores by register, as D893 did: a reload names another image through the same registers,
  and a store would write it into the wrong one.
