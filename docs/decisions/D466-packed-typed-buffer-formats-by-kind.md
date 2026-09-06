# D466 - Narrow typed-buffer formats translate through a packed path, built up one kind at a time


**measured** - 2026-09-02 (user-directed /loop: the GPU shader-translation frontier)

A typed buffer access whose components are narrower than a word - `8_8_8_8`, `16_16`, `10_10_10_2`
and the rest - packs several components inside one word, so it cannot go through `buffer_access`,
which moves a whole dword per component. It needs its own path: read the packed word, extract each
component's bit field, and convert it per the format's `ComponentKind`. That path,
`packed_buffer_memory`, is what `typed_buffer_memory` now hands the non-plain formats to instead of
refusing them outright.

**Built up by kind, lowest risk first, because a wrong conversion is invisible.** The reason the
format table is measured rather than transcribed is that reading a format wrongly does not fail - it
renders the wrong thing, silently, which is the one failure this project has no cheap way to catch
(the table's own file says so). A conversion written wrongly is the same hazard one level on. So the
path grows one `ComponentKind` at a time, each executed on a real device before the next is added:

1. **`UINT`, single word - done.** Its components are exact bit fields, so the whole operation is a
   `shift` and a `mask` per component and cannot round or normalise anything. `BUF_FMT_8_8_8_8_UINT`
   round-trips a stored word into four registers, low byte first, on a real GPU (`execute.rs`).
2. `SINT` next (needs an arithmetic shift-right for sign extension), then the normalised kinds
   (`UNORM`/`SNORM`: integer-to-float plus a divide and a clamp), then `FLOAT` (half unpack), then
   `SRGB`. Multi-word widths and packed stores are refused until each is built.

Everything still refused answers with a detail that says *component conversion*, so a shader needing
one is a loud gap in the trace rather than a quiet wrong render - the same honest-failure the refusal
this replaces already gave (its test is unchanged and still passes).

**Context, and a correction.** This began from a roadmap that listed G10 (the resource model) as "not
started". It is not: the V# buffer-descriptor decoder and the MUBUF/MTBUF-plain translation landed
weeks ago (worklogs 097/101, D203-D205), verified in the code before this was written. The roadmap
row was stale and is corrected. The narrow-format conversion here is the one capture-free piece of
G10 that genuinely remained.
