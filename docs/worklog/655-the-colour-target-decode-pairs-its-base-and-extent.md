# 655. The colour target decode pairs its base and extent - the piece the detile needs

**2026-09-17** - `CB_COLOR0_BASE` is decoded and paired with the extent into a `ColourTarget`, a step
of oops-libs' pipeline-state ask (inbox `-6e78`) and the register the detile (worklog 653) needs to
find a tiled surface in guest memory

## Why this register, of the ones 6e78 lists

`-6e78` asks for a set of render-state registers decoded into a "parsed pipeline state struct":
`CB_COLOR0_BASE`, `CB_TARGET_MASK`, the depth and stencil/blend state, and two UCONFIG registers. The
base is the one to take first, because it is the register with a consumer already waiting: the detile
landed in worklog 653 takes a surface's words and inverts the tiling, but *finding* those words in
guest memory needs the surface's base address, and nothing decoded it. `CB_COLOR0_BASE` is that
address. Paired with the extent (`colour_target_extent_at`, worklog 637) it is enough to place and
size colour buffer zero - where it is and how big - which is the minimum a host needs to make the
target resident and to read its pixels back.

## The measurement

`CB_COLOR0_BASE` is register index `0xA318` (`SET_CONTEXT_REG` base `0xA000` plus offset `0x318`).
Cited from oops-mesa's register database, not memory: `oops-mesa src/amd/registers/gfx103.json` maps it
at byte `167008` = `0x28C60`, which is context dword `(0x28C60 - 0x28000) / 4` = `0x318`. The value is
the base in 256-byte units - the same unit the image descriptor's base uses - so the byte address is
`value << 8`. obSCEne's `-a1f7` capture reads `0x02000e00` here for a surface it places at
`0x2000e0000`, which is exactly `value << 8` and corroborates the unit.

## What landed

`crates/orbistoun-gpu/src/registers.rs`:

- `const CB_COLOR0_BASE = 0xA318`, cited as above.
- `ColourTarget { base, width, height }` - the target as a submission set it up.
- `colour_target_at(writes)` - pairs the live `CB_COLOR0_BASE` and `CB_COLOR0_ATTRIB2` values;
  `None` unless *both* are set, because a base with no extent cannot be sized and an extent with no
  base cannot be placed, and pairing a real half with a guessed one is the plausible-output this
  refuses (D010). Most-recent-write-wins, the rule the file already uses.

Re-exported from `lib.rs`.

## Tests

Two, plus the extent decode's existing one:

- `the_colour_target_pairs_its_base_and_extent_and_refuses_without_either` - the `-a1f7` base value
  `0x02000e00` decodes to `0x2000e0000`, paired with a 64x64 extent; and both one-sided cases refuse
  (a guard watched rejecting, not just the happy path).
- `the_colour_target_takes_the_most_recent_base` - a rebound target takes the last base, like the
  extent.

## What 6e78 still wants

`DRAW_INDEX_2` is already decoded (worklog 628). Of the rest, `CB_TARGET_MASK` (`R_028238`, present in
the same Mesa database) is the next clean one; the depth/stencil/blend state and the two UCONFIG
registers each need their own Mesa-cited layout, and the verbatim acceptance names an obSCEne log not
in this tree - a synthetic or the in-tree gl-cube capture stands in for it (the `-26aa` precedent).
Each register is its own small unit; this took the one with a consumer waiting.

## Gate state

`cargo test -p orbistoun-gpu --lib` **63 passed, 0 failed** (+2); clippy clean; fmt clean;
`orbistoun-gpu-vulkan` builds on the new re-exports; identity scan exit 0. No commit.
