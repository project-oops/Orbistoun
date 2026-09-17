# 649. The texture detile is blocked on a disputed swizzle - do not implement it yet

**2026-09-16** - investigating the detile (the review said the 64KB_R_X swizzle was "measured") found
the opposite: the swizzle result is disputed and largely wrong, and building a detiler on it would
pass against linear data and be wrong on every tiled surface - so this records the block rather than
shipping the mistake

> **Update (2026-09-17):** the dispute recorded here was resolved in the swizzle's favour - obSCEne's
> own measurement pins the anchor to texel `(15,15)`, not the `(32,21)` this entry doubted, and three
> models agree. The detile is now implemented (worklog 653). This entry's core caution was right - do
> not build on a *disputed* swizzle - and it did its job: it drove the re-file (`-4d82`) that got the
> measurement clean. What changed is that the swizzle is no longer disputed.

## What the review believed, and what the ledger shows

Worklog 648 ended pointing at the texture detile as the clearest unblocked work, on the strength of
obSCEne request `-6e0f` being marked "delivered (settled per acceptance criteria)" with a measured
64KB_R_X swizzle point. Reading the sweep and the cross-project request ledger shows that resolution
does not hold:

- **The write probe failed.** `166-agc/tiling-swizzle` records `OBS|res|...|fail||tiling swizzle
  failed to execute`, with `fence-hit 0x0` (the fence never fired) and **every** `off-*` read-back
  `0x0`. Nothing was written and nothing was read. A guest compute/DCB store into tiled memory stalls
  the GE (the same behaviour `-6e0f` itself reported), so a swizzle **map** cannot be captured this
  way.
- **The one "verified" point is an assumption.** `-6e0f` point 3 claims the swizzle was confirmed from
  `-a1f7`'s draw: red at tiled byte 4348 against "linear offset 5504 for texel (32, 21)". But byte 4348
  on a linear 64-wide 32-bpp target is pixel `(63, 16)` - an ordinary linear position needing no
  swizzle - and nothing in the log names the texel that pixel came from; `(32, 21)` is a guess about
  where the draw landed. And `-a1f7`'s draw is **one point**, not a triangle (`-c7f3`,
  `VGT_GS_OUT_PRIM_TYPE = POINTLIST`), so it is one pixel that cannot supply a correspondence.

## The decidable test that would unblock it, not yet run

The target **is** tiled: `CB_COLOR0_ATTRIB3` (context offset `0x3b8`) is `0x08c6c000`, and
`COLOR_SW_MODE` at bits 18:14 decodes to `27` = 64KB_R_X (confirmed against Mesa's register database
and oops-sdk `gl_draw.c:365`). So the 4,096-pixel dump needs detiling before a texel can be read out
of it - the detile matters more, not less.

oops-sdk's tiler makes a falsifiable prediction: its basis vectors put texel `(15,15)` - and no other
texel in the block - at tiled index `0x43f` (byte 4348), where `-6e0f`'s `(32,21)` lands at `0x294`.
The one pixel `-a1f7` drew is at `0x43f`. So recovering that point's **intended** texel from its DCB
vertex data and viewport (both captured; the viewport transform - `PA_CL_VPORT_XSCALE`/`XOFFSET` =
32/32 at `0xA10F` - was mined in worklog 646) decides it in one step: `(15,15)` confirms the swizzle,
anything else refutes it. This is an open cross-project thread (`-4d82`, `-c7f3`, `-e91b`, oops-mesa's
`GB_ADDR_CONFIG` inversion), not something to pre-empt with a guess.

## The decision

**No detile is implemented.** A swizzle verified against a single pixel a linear layout already
explains is the plausible-output failure this project refuses (principle 3): it would pass a test
built from linear data and corrupt every genuinely tiled texture, the "works for forty thousand frames
then does not" shape. The honest states are two, and both are held here: the register decode of what a
texture *is* exists (the T#, worklog 642) and the tiling mode is now decodable (`COLOR_SW_MODE`, above);
the swizzle that would let its pixels be read waits on the decidable test above landing, on hardware,
in the thread already carrying it.

No obSCEne request is filed: `-4d82` already re-files exactly this, with the same acceptance (a surface
filled so each texel encodes its own `(x, y)`, read back with the texel-to-offset pairs, and the fence
made to retire).

## Gate state

No code changed - this is an investigation whose result is "do not build this yet". `cargo test
--workspace` unchanged and green; identity scan exit 0. No commit.
