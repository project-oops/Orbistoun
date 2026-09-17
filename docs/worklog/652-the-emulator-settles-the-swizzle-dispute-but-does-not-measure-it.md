# 652. The emulator settles the swizzle dispute but does not measure it - the detile stays blocked

**2026-09-17** - a reference emulator was used to decide whether the disputed 64KB_R_X swizzle is
right, and it is; but an emulator is an oracle, not a source, so the detile block from worklog 649
stands, and the useful output of the turn is a probe design filed to obSCEne, not a detiler shipped
here

> **Superseded, same day, by worklog 653.** This tick held the block too conservatively. The one
> measured anchor `(15,15)` → byte 4348, plus three-way model agreement, plus the full-table obSCEne
> sweep now in flight, is enough to proceed - the 32-bpp `64KB_R_X` detile is implemented in worklog
> 653, anchored on obSCEne's measurement (not the emulator) and refusing surfaces beyond the measured
> single-block case. The provenance reasoning below still holds; only the "do not implement" call was
> too cautious.

## What was asked, and the trap in it

The question was whether a reference emulator could decide the disputed swizzle "and give obSCEne a
hint so we can move forward, because hardware runs are expensive". It can decide it. KytyPS5's
`Gen5RenderTargetOffsetInBlock<uint32_t>` (`<emulators>`), a full emulator's 64KB_R_X 32-bpp
render-target swizzle, evaluated by hand puts texel `(15,15)` at byte `4348` (index `0x43f`) and
`(32,21)` at `0x294`. That matches oops-sdk's tiler and oops-mesa's `GB_ADDR_CONFIG` inversion
exactly, and matches the one hardware pixel obSCEne physically measured (`166-agc/primitive-draw`,
`color-idx 0x43f`, geometry-pinned to `(15,15)`). The dispute with `-6e0f`'s `(32,21)` is settled:
the swizzle is right.

The trap: "the swizzle is right" is not "the swizzle is measured". A first draft of this turn
transcribed the emulator's equation into `crates/orbistoun-gpu/src/tiling.rs` and cited it as a
hardware fact. That was withdrawn and the file deleted. Three software models agreeing is an oracle
telling us what "right" looks like; it is not orbistoun's own measurement, and shipping it would be
the plausible-output failure principle 3 forbids - a swizzle that passes every test built from
linear data and corrupts every genuinely tiled surface. The emulator removes the *doubt* about the
answer; it does not remove the *requirement* that orbistoun build only on measured facts.

## So worklog 649's block stands, for a better-understood reason

649 blocked the detile because the swizzle was disputed and rested on a single pixel a linear layout
also explains. The dispute is now resolved in the swizzle's favour, but the block does not lift,
because what is measured is still exactly one texel→offset pair, and a detiler is validated against
the whole table. The honest state is unchanged: the register decode of what a texture *is* exists
(the T#, worklog 642), the tiling mode is decodable (`COLOR_SW_MODE` = 27 = 64KB_R_X, 649), and the
swizzle that would let its pixels be read is confirmed-correct-but-unmeasured-as-a-table.

## The turn's actual output: a probe design obSCEne can measure the table with

The move-forward the request wanted is not orbistoun transcribing the equation - it is obSCEne
becoming able to return the table itself. The obstacle is specific and now understood: obSCEne's
map probe `166-agc/tiling-swizzle` stalls because it has the *guest* store into tiled memory
(`fence-hit 0x0`, all readbacks `0x0`, the GE never retires), whereas `166-agc/primitive-draw`
writes the *same* tiled surface through the colour backend and retires cleanly. The hardware is
willing to make a tiled write - through the CB, not through a shader store.

Filed to obSCEne (`-4d82`, cross-linked from `-c7f3`): draw a full-viewport TRISTRIP (the flip
`-c7f3` already asks for) with a fragment shader writing each pixel's packed `(x,y)`
(`(y<<16)|x` into the 32-bpp target), then scan the tiled buffer for one `off|byte|x|y` row per
dword. That is the 4,096-pair texel→offset table, measured, from one retiring draw with no store
into tiled memory. The three models become the *acceptance oracle* for that table - obSCEne's
measured rows diffed against the prediction - never the source. When those rows land, the detile
unblocks on measured data; until they do, it does not.

## Gate state

`tiling.rs` was created and deleted within the turn; no orbistoun source changed, so this is an
investigation-and-decision unit like 649. `cargo test --workspace` unchanged and green; identity
scan clean. The hint lives in the cross-project request ledger, not in either repository. No commit.
