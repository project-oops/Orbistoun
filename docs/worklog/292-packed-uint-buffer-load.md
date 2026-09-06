# 2026-09-02 - (/loop) Packed typed-buffer formats begin: UINT single-word load, GPU-verified

Pivoted (at the user's push) from the corpus-wall crunch to the ahead-of-guest GPU shader work. First
finding: the task the roadmap pointed at - G10 resource model, MTBUF translation - is **already done**
(V# decoder D204, MUBUF/MTBUF-plain translation worklog 101, D203/D205), verified in `model.rs`. The
roadmap doc was stale; corrected its G10 row.

The one capture-free piece that genuinely remained is **narrow-component format conversion** - the
case `typed_buffer_memory` refuses as "needing component conversion". Started it (D466), lowest risk
first:

- Added `packed_buffer_memory` in `orbistoun-translate/src/model.rs`. `typed_buffer_memory` now routes
  non-plain formats there instead of refusing; plain-word formats still go straight through
  `buffer_access` unchanged.
- Implemented **UINT, single word**: read the one packed word, and for each component shift it down and
  mask to its width - exact bit fields, no rounding, so it is correct by construction. Signed,
  normalised, float and sRGB kinds, multi-word widths, and stores are still refused, each with a detail
  that keeps the word "conversion" so the existing refusal test passes.
- Test: `a_packed_uint_load_extracts_each_component_from_one_word` stores a raw word (`0x04030201`) via
  the untyped path, loads it as `BUF_FMT_8_8_8_8_UINT`, and asserts each byte lands in its own register,
  low byte first. It **executed on a real Vulkan device** (no `!! SKIPPED` printed), so the extraction
  is validated on hardware, not just `spirv-val`-accepted.

clippy/fmt clean; all 104 execute tests plus the rest of the translate suite pass; the three existing
typed-format refusal tests are unchanged and green.

Next increments, each device-validated before the next: **SINT** (needs a `SHIFT_RIGHT_ARITHMETIC` op
for sign extension - check it exists in `orbistoun-spirv::op`, add if not), then **UNORM/SNORM**
(int->float convert, divide by the max, clamp for SNORM), then **FLOAT16** (half unpack), then
**SRGB**; then packed **stores** (pack + write), then multi-word widths. The `execute.rs` mtbuf/mubuf
helpers and this test are the template.
