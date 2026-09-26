# D692 - A stored image declares no format

**Status:** assumed
**Date:** 2026-09-15

A guest image store translates to a storage image declared with format `Unknown` and the
`StorageImageWriteWithoutFormat` capability, with the matching device feature requested where
offered. A device without the feature is reported before a pipeline is built.

**Why:** the guest's format is in a descriptor the translator does not decode (D690), so any
named format would be invented, and a wrong one silently reinterprets every texel. `Unknown`
says the shader does not know, which is true. Sampled and stored images are separate host
bindings, so a shader that stores and then samples one texture is not served.

**Rejected:**
- Naming a format such as `Rgba8`: silently wrong for every other format.
- Refusing the store: unnecessary once a format-free store needs no guess.
