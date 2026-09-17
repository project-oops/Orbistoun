# D702 - Render targets are identified by extent, not their disputed base address

**Status:** decided
**Date:** 2026-09-16

## The choice

A colour render target became a resource the frontend makes resident (D701): a `SetRenderTargets`
names it by a `ResourceId`, and the backend sizes its attachment from the dimensions that id carries.
The choice is **what that id is derived from**. A render target's true guest identity is its base
address - two draws to one address are one target - so the obvious id is the address. This decides
against that, for now: a target is identified by its **extent** (width x height), tagged into a
reserved region of the id space.

```rust
fn colour_target_id(extent: ColourTargetExtent) -> ResourceId {
    const TARGET_NAMESPACE: u64 = 1 << 63;
    ResourceId(TARGET_NAMESPACE | (u64::from(extent.width) << 16) | u64::from(extent.height))
}
```

## Why the extent and not the address

The two registers are not equally certain. The size register, `CB_COLOR0_ATTRIB2` (offset `0x3B0`),
is corroborated by both sources this project has: the hardware capture
(`agc-gl-cube-fw1240-a`) writes it for a 1920x1080 frame the console hashed, and obSCEne's constructed
draw stream (`tests/graphics_draw.rs`, sweep 9a41) writes it for a 64x64 one - and the decode returns
exactly those dimensions from each (worklog 637). The base register, `CB_COLOR0_BASE`, is **disputed**:
the capture puts it at offset `0x318`, obSCEne's stream at `0x200`. Keying the id on the address would
rest the target's whole identity on the less certain of the two registers, and a wrong base offset is
the silent kind of wrong - the id would still be a plausible number, just the wrong one.

So the id is keyed on what is corroborated. The cost is that two same-sized targets at different
addresses collide to one id - but that is harmless under what the backend does with a target today.

## Why the collision is harmless today, and when it stops being

The executor draws into an attachment it allocates and reads straight back; it stores **nothing**
per-target between draws. So the only thing a target id carries is a size, and two same-sized targets
being one id means they share a size lookup - which is correct. The moment a backend keeps a host
object per target (a persistent `VkImage` a later frame samples), address becomes load-bearing:
same-size-different-address would then alias two real images. At that point the id must key on the
address - by then decoded from the base register whose offset a real multi-target capture will have
settled - and this decision is where to look for why it changed.

## Why the top-bit namespace

Shader (and, later, buffer) ids are minted sequentially from one and stay small. Tagging a target id
into the top bit of the 64-bit id space keeps the two disjoint with no shared counter and no
coordination - a target and a shader can never collide, by construction rather than by luck. The
extent packs into thirty bits (each dimension is at most the register's fourteen), so the tag is free.
This is cheaper and more honest than a shared `next_resource` counter, which would also break
idempotency: a fresh id every submit would make the backend re-record the same target every frame.

## What this commits to, and what it leaves open

Committed: a colour target is a residency resource sized from `CB_COLOR0_ATTRIB2`, identified by its
extent in a tagged id namespace, made resident by the driver and selected by `SetRenderTargets`.
Left open (named, not built): the target's **address** as its identity (waiting on the disputed base
register), the **clear colour** (no register oracle exists - neither capture writes one, so the
attachment stays interim black rather than inventing an offset), **depth** targets, and **per-draw**
target changes (one target per frame is decoded, which is all any oracle in hand shows).
