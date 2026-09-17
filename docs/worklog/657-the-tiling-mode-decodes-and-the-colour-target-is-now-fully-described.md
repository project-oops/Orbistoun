# 657. The tiling mode decodes - the citation held up, and the colour target is now fully described

**2026-09-17** - `COLOR_SW_MODE` is decoded into a `ColourSwizzleMode`, the register the detile needs
to know its swizzle applies; the field citation worklog 656 could not find turned out to be in the
type-definition block that grep had truncated past

## The citation worklog 656 gave up on was there

Worklog 656 deferred this decode because `COLOR_SW_MODE`'s bit range would not resolve to a source: a
grep of `gfx103.json` found no such field, `gl_draw.c:365` had drifted to depth registers, and the
neighbouring generations disagreed (`gfx9` `[18,22]`, `gfx12` `[15,17]`). That grep was truncated
before it reached the answer. The `CB_COLOR0_ATTRIB3` **type** definition in the same file
(`oops-mesa src/amd/registers/gfx103.json:11505`) lists its fields, and line 11509 is
`{"bits": [14, 18], "name": "COLOR_SW_MODE"}`. So worklog 649's `[14,18]` was right and is citable
after all. The lesson worth keeping: a register db separates the address *map* from the field *type*,
and a field absent from the map is not a field absent from the file - read the type block before
concluding it is uncited.

The enum value is cited too: `oops-mesa src/amd/addrlib/inc/addrtypes.h:225` (`AddrSwizzleMode`) puts
`ADDR_SW_LINEAR` at `0` and `ADDR_SW_64KB_R_X` at `27`, and the `-a1f7` capture's `0x08c6c000` decodes
through bits `[14,18]` to `27`.

## What landed

`crates/orbistoun-gpu/src/registers.rs`:

- `CB_COLOR0_ATTRIB3` = register `0xA3B8` (cited: `gfx103.json` byte 167648).
- `ColourSwizzleMode` - `Linear`, `Tiled64KbRX`, or `Other(u32)`. Only the two orbistoun can act on are
  named; every other mode is carried by its raw `COLOR_SW_MODE` value, so an unmodelled tiling is
  refused downstream (D010) rather than read with the wrong swizzle.
- `decode_colour_swizzle_mode(attrib3)` and `colour_swizzle_mode_at(writes)` - the latter `None` when
  unset, because a surface of unknown tiling must not be assumed linear *or* tiled. Re-exported.

## Why it matters: the colour target is now fully described on the decode side

The three registers a host needs to place, size and read colour buffer zero are now all decoded, and
together they close the loop the detile opened:

- **base** - `CB_COLOR0_BASE` (worklog 655): where the surface is in guest memory.
- **extent** - `CB_COLOR0_ATTRIB2` (worklog 637): how big.
- **tiling** - `CB_COLOR0_ATTRIB3.COLOR_SW_MODE` (this): whether `crate::tiling`'s `64KB_R_X` detile
  applies (`Tiled64KbRX`), the surface is linear (`Linear`), or it is a mode to refuse (`Other`).

The host upload path - not built yet - now has everything it needs to decide, from the stream alone,
whether and how to detile a captured surface: read the mode, and dispatch to `detile_64kb_rx_bpp4`
only on `Tiled64KbRX`.

## Gate state

`cargo test -p orbistoun-gpu --lib` **67 passed, 0 failed** (+2); clippy clean; fmt clean;
`orbistoun-gpu-vulkan` builds; identity scan exit 0. No commit.
