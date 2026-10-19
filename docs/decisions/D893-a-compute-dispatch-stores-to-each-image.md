# D893 - A compute dispatch stores to each image its descriptors name

**Status:** decided
**Date:** 2026-10-19

A compute module may store to up to four images. Each one is identified by the registers that hold
its descriptor, as D690 identifies a sampled texture. A store through a descriptor no earlier store
named is a new storage image: the first at binding three, the rest at 21, 22 and 23, after the last
texture binding. A store through a descriptor an earlier store named is that image again.

The translation reports where each stored image's descriptor came from, in the order the program
first stores to them. A dispatch resolves, uploads, reads back and writes back each one
independently, through its own surface, channel order and compression keys. All of a module's
storage images are declared as one numeric type. A dispatch whose stored images mix integer and
float is refused by name.

A pixel shader still stores to one image, its harness's. Kept translations written when a dispatch
stored to one image at most still load, as a list of that one image or of none.

**Why:** Grand Theft Auto V's scene compute program reads an image, writes it back, and stores to
a second one. Each stored image's descriptor is resolved the same way a single one was, so binding
more of them leaves nothing unknown. Vulkan guarantees four storage images per stage
(`maxPerStageDescriptorStorageImages`). A device test copies one image into two through `s[8:15]`
and `s[16:23]`, and both come back equal to the source.

**Rejected:**
- Refusing a second stored image, as D690 did when only one storage binding existed: the
  descriptors say which image is which, so nothing has to be guessed.
- Typing each storage image separately: no program met so far mixes the two kinds, and a mixed one
  is refused rather than written as the wrong kind.
