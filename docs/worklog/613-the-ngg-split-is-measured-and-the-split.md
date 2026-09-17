# 613. The NGG split is measured, and the split obSCEne stated is refuted by its own draw

**2026-09-15** - consuming `REQ-20260915T1652Z-6fad`, sweep `20260915-192617`

The one assumption in the mesh translation (D688) is now measured. The low field of the
`MSG_GS_ALLOC_REQ` payload is the vertex count and the high field is the primitive count - which
is how `MESH_COUNT_BITS = 12` already read it, so **no code changed; an assumption became a
measurement.**

## What finally measured it

The last two "resolutions" printed a static table (worklog 602). This one submitted a real draw.
`check_agc_ngg_primitive_draw_m0` in obSCEne is a copy of the working `check_agc_primitive_draw`
with one dword - the `m0` literal - swept over three values, each submitted, fenced and read back:

| `m0` | low 12 = verts, high = prims | fence | colour |
|---|---|---|---|
| `0x1003` | 3 verts, 1 prim (exact) | `0xbeefcafe` hit | red drawn |
| `0x1004` | 4 verts, 1 prim (surplus verts) | `0xbeefcafe` hit | red drawn |
| `0x3001` | 1 vert, 3 prims (starved) | `0x11111111`, unhit | background, shader never ran |

Starving the **low** field stops the draw; inflating it does not. So the low field is what the
body's three vertices have to fit inside - the vertex count - and the primitives are above it.
That is the direction the translator assumed, now with a submitted frame behind it.

## obSCEne's own stated conclusion is wrong, and its own row proves it

The resolution's headline is *"`prims = m0 >> 16`, `verts = m0 & 0xffff`"*. That is a sixteen-bit
split, and it contradicts variant A:

```
0x1003 >> 16 = 0 primitives      -> would draw nothing
0x1003 & 0xffff = 4099 vertices
```

Variant A **drew a triangle**. Under a sixteen-bit boundary it declares zero primitives and could
not. So the boundary is at or below bit 12, not 16. The rows are sound and hard-won; the sentence
written over them is not, and it is the sentence another project would copy.

The correct reading, checked against all three draws, is the twelve-bit one - low field vertices,
high field primitives:

```
A 0x1003 boundary=12: verts=3 prims=1 -> draws     boundary=16: prims=0 -> NO DRAW (but it drew)
G 0x1004 boundary=12: verts=4 prims=1 -> draws     boundary=16: prims=0 -> NO DRAW (but it drew)
F 0x3001 boundary=12: verts=1 prims=3 -> NO DRAW   boundary=16: prims=0 -> NO DRAW
```

Only the twelve-bit column matches what the console did in all three rows.

## What is measured and what is not

**Measured:** the low field is vertices, the high field is primitives. A shader that starves the
vertex count faults before drawing.

**Not uniquely pinned:** the exact boundary. Three samples fix it only to the range bits 3..12 -
`0x1004`'s vertex `4` needs the boundary at bit 3 or above, and `0x1003`'s primitive `1` needs it
at bit 12 or below. Twelve is kept because it reads oops-sdk's own `0x1003` as exactly one
primitive of three vertices, which is how that shader encoded it, and because real NGG counts
(workgroups of a few dozen vertices) never approach 2^12, so the choice within 3..12 cannot affect
any real translation. Pinning it exactly would need an `m0` like `0x0800`, and it would change
nothing, so it is not worth a console run.

## Filed

- `REQ-20260915T1846Z-557c` in obSCEne's inbox: the data is correct, the stated `>> 16` formula is
  not, here is the reading its own rows support - so a consumer does not encode primitives at bit
  16. No re-run asked for.

## Files

- `crates/orbistoun-translate/src/model.rs` - `MESH_COUNT_BITS` comment, now a measurement.
- `docs/decisions/D688-...md` - the payload section, rewritten from "not yet known" to measured.

## Next

The mesh translation has no remaining assumptions. The path from here is the render target the
draw needs, which is capture-shaped (`REQ-20260915T0929Z-31de`).
