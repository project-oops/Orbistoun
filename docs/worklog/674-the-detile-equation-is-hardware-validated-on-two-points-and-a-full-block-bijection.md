# 674. The detile equation is hardware-validated on two points and a full-block bijection

**2026-09-17** — obSCEne delivered `-db54`, the whole texel→offset table for a `64KB_R_X` 32-bpp
macro-tile, filled through the render path. It upgrades the detile swizzle's provenance from a single
anchor to two independent hardware points plus a confirmed bijection - and orbistoun's equation, derived
to fit the one anchor, already matches all of it.

## What db54 confirmed

`166-agc/tiling-swizzle` (sweep `20260917-124503`) on live hardware:
- `(15, 15)` → byte 4348 (the existing anchor).
- `(32, 21)` → byte 2640 - a **second** point, and the one the withdrawn `-a1f7` reading had wrong
  (it had claimed `(32, 21)` → 4348).
- bijective across all 16,384 texels of the 128x128 block.

## Orbistoun already reproduces it

`tiled_byte_offset_64kb_rx_bpp4(32, 21)` computes row `0x250` ⊕ column `0x800` = `0xa50` = **2640**,
exactly the second measured point - with no change to the equation, which was derived to fit only
`(15, 15)`. That an independently-measured second point falls out of the closed form is the strongest
evidence yet that the form is right, and it is what `-db54` set out to check.

## What changed here

- `the_measured_anchor_is_texel_fifteen_fifteen` is now
  `the_measured_anchors_are_two_hardware_points`: it asserts both `(15, 15)` → 4348 and `(32, 21)` →
  2640, and keeps the negative that `(32, 21)` is not the withdrawn 4348.
- The equation's doc and the module provenance note move from "one texel→offset pair, and obSCEne is
  *being extended* to return the whole table" to the delivered fact: two points and a full-block
  bijection, `-db54`. The `-4d82` "table lands in the future" wording is retired - the table landed.

## What this does and does not unlock

It hardens the swizzle's foundation - the intra-block equation is now measured-confirmed rather than
anchored-and-assumed. It does **not** extend the detile's *reach*: `detile_64kb_rx_bpp4` still refuses a
surface past one 64 KiB block, because the block-level tiling and `pipeBankXor` for multi-block surfaces
are a different measurement `-db54` did not cover (it is one macro-tile). And it is still 32-bpp
`64KB_R_X` only. So the equation is now trustworthy where it applies; where it refuses, it still refuses.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 81 passed; fmt clean;
`./bin/orbistoun prose` exit 0; identity scan clean. No commit.
