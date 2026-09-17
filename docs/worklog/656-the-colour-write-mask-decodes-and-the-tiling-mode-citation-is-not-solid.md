# 656. The colour write mask decodes; the tiling-mode citation turned out not to be solid

**2026-09-17** - `CB_TARGET_MASK` is decoded into per-target channel masks (another of oops-libs'
`-6e78` registers), after the register I meant to do first - the tiling mode - failed to produce a
citation I would stand behind

## The pivot, and the finding behind it

The plan was to decode `CB_COLOR0_ATTRIB3`'s `COLOR_SW_MODE` (the tiling mode), because it has a real
consumer: the detile (worklog 653) handles `64KB_R_X`, and the tiling mode is what says whether that
is the right swizzle for a given surface. The address citation is solid (`CB_COLOR0_ATTRIB3` at
oops-mesa `gfx103.json` byte 167648 = register `0xA3B8`). **The field is not.** Worklog 649 put
`COLOR_SW_MODE` at bits `[14,18]`, "confirmed against Mesa's register database". Checked against the
tree:

- `gfx103.json` does **not** define `COLOR_SW_MODE` at all - it maps `CB_COLOR0_ATTRIB3`'s address and
  gives no fields. The neighbours disagree with 649 and each other: `gfx9.json` has it at `[18,22]`,
  `gfx12.json` at `[15,17]`.
- The other citation, `oops-sdk gl_draw.c:365`, is now a loop over depth-buffer registers - the line
  reference has drifted since 649 was written.

The arithmetic still works out (`(0x08c6c000 >> 14) & 0x1f` = 27 = `64KB_R_X` for the `-a1f7` value),
so `[14,18]` is probably right - but "probably right from a citation that no longer resolves" is the
from-memory register hazard this project has been bitten by before ([[register-offsets-in-requests-come-from-mesa]]).
So the tiling-mode decode waits for a citation that holds, rather than being written on a shaky one.
Recorded here so the next tick does not re-derive the doubt from scratch.

## What landed instead

`CB_TARGET_MASK` is cleanly cited and was done in its place. `oops-mesa src/amd/registers/gfx103.json`
maps it at byte `164408` = `0x28238` (register `0xA08E`) **and** defines its eight four-bit fields
`TARGET0_ENABLE`..`TARGET7_ENABLE` at `[0,3]`..`[28,31]` - both address and layout from the same file.

`crates/orbistoun-gpu/src/registers.rs`:

- `TargetMask { targets: [u32; 8] }` - one four-bit channel mask per MRT (bit 0 red .. 3 alpha), with
  `writes_target(i)` and `active_targets()`.
- `decode_target_mask(value)` and `target_mask_at(writes)` - the latter `None` when the stream never
  set it, because which targets are written is not a thing to default. Re-exported from `lib.rs`.

Two tests: the per-target channel decode (MRT0 `0xF`, MRT1 red+alpha `0x9`, MRT2 off), and the
live-register read with a most-recent-wins rebind and an absent-when-unset guard.

## Gate state

`cargo test -p orbistoun-gpu --lib` **65 passed, 0 failed** (+2); clippy clean (an unused-import
warning in the test module was cleared); fmt clean; `orbistoun-gpu-vulkan` builds; identity scan exit
0. No commit.
