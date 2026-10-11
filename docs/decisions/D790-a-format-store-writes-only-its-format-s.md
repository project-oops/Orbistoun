# D790 - A format store writes only its format's components

**Status:** decided
**Date:** 2026-10-11

A `buffer_store_format_*` with more channels than its descriptor's whole-word format has
components writes the format's components, taken from the first channels, and leaves the rest of
the element as it was. The descriptor's selects must still be the identity over the components it
writes. Fewer channels than components, or more through a packed format, stays refused by name.

Assumed (`known_by = assumed`), grounded in Mesa: its GL image-buffer store hands
`buffer_store_format` four channels whatever the texel buffer's format, expanding three to four
(`ac_nir_to_llvm.c:2281-2293`). An `r32ui` image buffer is stored with a vec4 through a
one-component descriptor, and that is correct only if the store writes the format's one
component and nothing past it.

**Why:** PPSA25872's fill program stores `xyzw` through a `32_UINT` descriptor (word3 `0x14004`,
selects `X, 0, 0, 0`), and the translation refused the mismatch, stopping its first submission at
the dispatch at byte 0x258.

**Rejected:**
- Writing all four channels into the element: a 32_UINT element is one word, so the other three
  would land on the next elements, which Mesa's image stores would corrupt.
