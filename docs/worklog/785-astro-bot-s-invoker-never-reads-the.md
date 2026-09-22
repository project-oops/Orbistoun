# 785. ASTRO BOT's invoker never reads the arena-init pointer, closing the runtime approach; the loop redirects to the buildable AGC synthesis path for the leading titles

**2026-09-21** — worklog 784 dropped the loop to a heartbeat with the retail frontier tool-bound. The
operator re-fired it, which reads as "keep making real progress", so this closes the last ASTRO BOT
check and redirects to the one remaining path that can move a wall *without* a new tool or a hardware
measurement: AGC synthesis, which worklog 529 used to clear four fault sites.

## The last ASTRO BOT check

A read-watchpoint on the **runtime** arena-init pointer, `image+0x88dc158:rw` (the RELRO slot the
RELATIVE reloc populated with `base + 0x27cc20`), came back **`never touched`**. So no guest code reads
the pointer before the fault - the invoker never runs; the startup diverges upstream of it. That is the
expected close to the native-title line: reaching the divergence needs an execution trace from entry
(worklog 783/784), which is a build, not a tick. ASTRO BOT stays paused there.

## Why AGC synthesis is the productive pivot

Three of the six titles (PPSA02664, PPSA03416, PPSA28061) wall on **AGC functions that are not retail
exports** - inline SDK helpers this build emitted out-of-line (worklog 723). obSCEne cannot call them,
so they are orbistoun's to **synthesise** from lawful sources, and that has a track record: worklog 529
decoded `sceAgcCreateShader`'s in-place object model from obSCEne's measured `0x2c` bytes plus disassembly
and cleared four consecutive fault sites. Unlike the native titles' computed-address invocations, an AGC
return value is a data structure that can be built and checked.

The data to build from exists in-tree: `orbistoun-gpu/src/registers.rs` (the register file),
`crates/orbistoun-gpu/data/packets.toml`, and SELFish's clean-room `data/agc-shader-format.tsv`. The
cleanest target is **`sceAgcGetRegisterDefaults2`** (PPSA28061's exact wall, worklog 769): a named
function that returns a pointer to a table of GPU context-register reset defaults, which the title
dereferences at `+0x38`. Register reset defaults are a documented GPU fact (Mesa/ISA), not console-derived,
so the table is sourceable; the layout the title expects is REable from its own use of the pointer.

## The plan

A multi-tick synthesis, taken to an implementation rather than a characterisation:

1. Disassemble PPSA28061's call site `image+0x10b9e9` and how it consumes the returned pointer (which
   fields at which offsets, how `+0x38` is used) to fix the structure the title needs.
2. Source the default values from `registers.rs`/Mesa's reset state for the register set it asks for
   (`arg0 = 0xd`), recorded with `known_by` citing the lawful source, not a guess.
3. Wire `sceAgcGetRegisterDefaults2` in `orbistoun-gpu` to return a pointer to that table, and run
   PPSA28061 to measure whether the wall moves - the loop's actual objective.

This is buildable now, needs no hardware slot, and if it moves PPSA28061 it is the method that also
addresses PPSA02664/03416's descriptor wall.

## Gate state

No code changed - a redirect that closes ASTRO BOT's runtime line (the invoker never reads the arena-init
pointer) and commits the loop to synthesising `sceAgcGetRegisterDefaults2` for PPSA28061 from in-tree
register data and the title's own structure use, per worklog 529's proven method. `./bin/orbistoun check`
green, worklog index regenerated, identity scan clean. No commit.
