# 667. The texture detile refuses a non-32-bpp format instead of mis-tiling it

**2026-09-17** — the detile applies a 32-bpp (`bpp4`) swizzle, but `detile_texture`/`detile_image`
took the descriptor's format on trust and detiled anything - a 16-bpp or block-compressed texture
would have been silently mis-tiled, every texel read across or within its element. inbox `-8c34`'s
last open item ("for textures, a format-to-bpp check so a non-32-bpp surface is refused rather than
mis-tiled") is now closed: the entry points refuse a format that is not 32 bpp.

## The guard, and where the sizes come from

`element_bytes(format)` maps a GFX10 `IMG_FORMAT` code to its bytes-per-texel, cited from
`oops-mesa src/amd/registers/gfx10-rsrc.json`, which lists the `GFX10_FORMAT_*` enum with values and
lays the standard formats out in ascending element size. So the sizes are value ranges: `1..=6` one
byte, `7..=19` two, **`20..=61` four** (`32`, `16_16`, `10_11_11`, `11_11_10`, `10_10_10_2`,
`2_10_10_10`, `8_8_8_8`), `62..=71` eight, `72..=74` twelve, `75..=77` sixteen, and the `8`/`8_8`/
`8_8_8_8` sRGB trio at `128`/`129`/`130`. Every other code - the packed 16-bit, depth, subsampled,
FMASK and block-compressed formats past 127 - is left `None` and refused rather than assigned a size
from its name; the detile only needs to tell the 32-bpp case from the rest, and refusing is the safe
direction.

`detile_image` (raw pixels) and `detile_texture` (out of a guest window) now both refuse a format
whose `element_bytes` is not four: `DetileError::UnsupportedFormat { format }`, and for
`detile_texture` wrapped as `SurfaceError::Detile(..)`, returned before the window is even touched.

## The check surfaced a wrong test placeholder

The two `detile_texture` tests built their descriptor with `format: 0x0A`. Under the field's actual
encoding (`format = (word1 >> 20) & 0x1FF`, the GFX10 `IMG_FORMAT`) `0x0A` is `16_USCALED`, a two-byte
format - the value was carried over from the older GCN `DATA_FORMAT` enum, where `8_8_8_8` is `0x0A`.
The guard flags it, which is the point. Both now use `56` (`GFX10_FORMAT_8_8_8_8_UNORM`), the 32-bpp
format those textures actually are.

## Tests

`detile_texture_refuses_a_non_32_bpp_format` drives a `16_16_16_16_FLOAT` (`71`, eight-byte)
descriptor and asserts the refusal - the negative that makes the check real, since without it the
descriptor would detile as 32 bpp. `the_format_element_size_matches_the_cited_table` pins the
boundaries the detile turns on: `56`/`50`/`36` are four bytes, `19` just below the block is two, `62`
just above is eight, `130` (sRGB) is four, and `169` (`BC1_UNORM`) and `0` (`INVALID`) are refused
rather than sized.

## What is still open on 8c34

The swizzle *equation* remains 32-bpp `64KB_R_X` only (`4KB_S`, other bit-depths and the BC
pass-through wait on their measurements), and the backend consuming a `Surface` as a Vulkan texture
is the host half. What closed here is the safety guard between them: a non-32-bpp surface is refused,
not mis-read.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets -- -D warnings` clean; gpu lib 75 passed; fmt clean;
`./bin/orbistoun prose` exit 0; identity scan clean. No commit.
