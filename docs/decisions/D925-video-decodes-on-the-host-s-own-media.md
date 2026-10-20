# D925 - Video decodes on the host's own media decoder

**Status:** decided
**Date:** 2026-10-20

A title's H.264 and HEVC video decodes on the decoder the host operating system ships. On Windows
that is Media Foundation's synchronous software decoder transform, found by `MFTEnumEx` for the
codec and read as NV12. It lives in `orbistoun-media-host`, which knows nothing of the guest, as
`orbistoun-audio-host` is the host side of audio. `orbistoun-video` places the pictures in the
console's layout. On a host with no such decoder the open is refused by name and the video does
not play.

The decoded samples are not a choice. H.264 clause 8 and H.265 clause 8 define the decoding
process exactly, so every conforming decoder produces the same bytes. The tests compare each
picture with a second, independent decoder's output for the same synthetic stream. Only the
layout around the samples is the console's (pitch, alignment, plane offsets, which allocation
holds them), and that comes from obSCEne (REQ-20261020T1135Z-vd01).

**Why:** five of the six library titles play video through libSceAvPlayer, and PPSA02664's and
PPSA04263's intros wait on it. orbistoun is MIT OR Apache-2.0 and `deny.toml` allows no copyleft,
which rules out the decoder most emulators link.

**Rejected:**
- Linking ffmpeg's libavcodec. It is LGPL or GPL depending on the build, outside the licence
  list, and a large native dependency on every platform.
- The `openh264` crate (BSD-2-Clause, building Cisco's C decoder). It covers H.264 only, so HEVC
  would still need a second decoder, and it adds a C build on every platform.
- A decoder written here. Conforming H.264 and HEVC decoders are large, and a wrong one is
  silently wrong. The host's decoder is maintained and tested by the platform's vendor.
- A hardware decoder transform. They are asynchronous, and the guest library's calls are
  synchronous.
