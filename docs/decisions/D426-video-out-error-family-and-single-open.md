# D426 - Video-out answers its own error family, and refuses a second open of a held output

**Status:** decided
**Date:** 2026-09-01

Every video-out call refuses with the vendor video-out error base, not the kernel's placeholder.
Opening the main output while it is already open is refused rather than granted a second handle.

**Why:** A guest that tests a video-out failure against the vendor's own error family never
matches a kernel-shaped placeholder. A live output tracks its open state and identity, so a
second open of it is refused exactly as the platform refuses one, and a probe that expects that
refusal skips rather than passing where it should not.

**Rejected:** answering the generic kernel error family for every subsystem - a guest testing the
vendor's own error code for its subsystem never matches it.
