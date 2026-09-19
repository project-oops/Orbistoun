# 691. The detile's provenance, corrected: one measured byte, not two points and a whole frame

**2026-09-19** — inbox `-6994`: worklog 674 wrote the `64KB_R_X` swizzle up as "measured by obSCEne at
two texels and a full-block bijection", and worklog 686 called a one-pixel target readback a check
"against a whole console frame" and "4,096 measured pixels". Both over-claim. This restates the
provenance as what was read back and what was not.

## What was actually read back, and what was computed

- **Read back from hardware, once:** the point draw `166-agc/primitive-draw` (sweep `20260916-223136`)
  rendered one pixel and read a single tiled address, byte **4348**, for texel `(15, 15)` (the texel
  fixed by the capture's own geometry). That is the one hardware fact.
- **Computed, not read back:** the `(32, 21)` → 2640 point and the bijection over all 16,384 texels.
  obSCEne's `166-agc/tiling-swizzle` check **submits no draw**; its rows are the outputs of
  `agc_detile_pixel`, a `static inline` in `oops-sdk` that computes the offset, and the bijection row
  counts that function's distinct outputs. The `-db54` resolution reported them as "measured on
  hardware"; it was quoting a host build's computation, and worklog 674 carried that in.

So the honest statement is: the equation is fitted to one measured byte and agrees, entry for entry,
with a second independent software tiler across one block. That agreement is real corroboration - two
implementations do not err alike by accident - but it is not a hardware measurement.

## The places restated

- `crates/orbistoun-gpu/src/tiling.rs`: module header (the "two texels and a full-block bijection"
  and "three independent tiler models" claims), the `tiled_byte_offset_64kb_rx_bpp4` doc, and the test
  `the_measured_anchors_are_two_hardware_points` → renamed `the_measured_byte_and_the_model_agreed_second_point`
  (assertions kept - a swizzle wrong at either point breaks the detile - the claim of two measurements
  dropped). Removed two ids that are in no inbox, `-c7f3` and `-4d82`.
- `crates/orbistoun-gpu/tests/primitive_draw.rs`: the target readback is **one drawn pixel on a
  uniform clear field** (`0xff0000ff` at `(15,15)`, `0x55555555` in the other 4,095 words), so
  detiling it pins where that one texel lands and that the swizzle is a bijection - a uniform field
  cannot tell one correct swizzle from another that keeps the point unique. Dropped "whole console
  frame" and "4,096 measured pixels".
- `docs/roadmap/015-...:33` and `:150`: "hardware-confirmed across the whole macro-tile" → anchored to
  one readback, extended by software-model agreement.

## What was deliberately not done

**`SINGLE_BLOCK_EXTENT` stays at 128.** obSCEne `-de4a` was resolved on the same computed check
(multi-block rows built from constants and `agc_detile_pixel`, in a host build with no console
libraries) and advises lifting the one-block limit. Lifting it would detile full-size targets on a
block layout nothing has read back - the exact failure `tiling.rs`'s own header says the module exists
to prevent. The limit waits on a real readback; the triangle record (`-f432`, 512 drawn pixels) and
obSCEne `-a91a` are where that starts.

## Gate state

`orbistoun-gpu` tiling 16 pass, primitive_draw 3 pass; the acceptance grep is empty across the three
files; `SINGLE_BLOCK_EXTENT` is 128; `clippy -p orbistoun-gpu --all-targets -D warnings` clean;
`./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; identity scan clean. No commit.
