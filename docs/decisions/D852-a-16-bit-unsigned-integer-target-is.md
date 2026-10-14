# D852 - A 16-bit unsigned integer target is drawn through a 32-bit one and written back as it is held

**Status:** decided
**Date:** 2026-10-14
**known_by:** assumed

A colour target whose `CB_COLOR0_INFO` names `COLOR_16`, `NUMBER_UINT` and `SWAP_STD` holds one
16-bit unsigned integer a texel, red, two bytes. It lies by its own layout: for `64KB_R_X`,
256 x 128-texel blocks with addrlib's 16-bpp equation (`tiling.rs` `RX_BPP2_X_BITS`). Its texels
are read out of memory each as the low half of a word. It is drawn through a Vulkan `R32_UINT`
attachment, one word a texel, which the pixel shader's `uvec4` output writes (D851). Write-back
stores each word's low half. A draw into it writes red alone. Any other swap stays refused.

A colour target that holds integers, `NUMBER_UINT` or `NUMBER_SINT`, bypasses blending whatever
its `CB_BLENDn_CONTROL` enables.

**Why:** `COLOR_16` is what Mesa targets a one-channel 16-bit format at (`ac_formats.c` 486), and
`SWAP_STD` on one channel is red (830). radeonsi sets `BLEND_BYPASS` for `NUMBER_UINT` and
`NUMBER_SINT` (`ac_descriptors.c` 1432-1436), and Vulkan forbids blending an integer attachment.
Grand Theft Auto V draws into a 3840 x 2160 target of it.

**Assumed:** that an integer the pixel shader exports into this target fits sixteen bits, as a
`UINT16_ABGR` export format packs it, so storing the low half keeps it. A wider value is
truncated here; what the console's colour block does with one is unmeasured.

**Rejected:**
- An `R16_UINT` attachment: its frame would be half a word a texel, where every other target's is
  a word or more, and the write-back would need its own path. A 32-bit attachment holds every
  16-bit integer exactly.
- Drawing it through an eight-bit attachment: an integer output cannot be written to a
  normalised attachment.
