# 2026-09-01 - (/loop) Made reservation failures legible, then cracked the collision they hid

Picked up worklog 284's open PPSA04263 wall by building the diagnostic it asked for, rather than
guessing further. Two parts (D462): Windows `platform::reserve` now distinguishes the `VirtualAlloc`
null cause via `GetLastError` (`ERROR_INVALID_ADDRESS` -> conflict, else host-refused with the
code - the D010 rule Unix already followed), and `AddressSpace::reserve`/`protect` record their
last failure - base, length, reason - into allocation-free statics the worker's `persist` prints
once from the host side (D381-safe).

It paid off on the first run: PPSA04263 reported `base=0x720000000000 len=0x32000000 - conflict`,
and `0x720000000000` is `SUGGESTED_DATA_BASE` exactly. `MAPPING_BASE` and the thunk data-block base
were **the same address** - the data blocks are reserved there at load, so the first guest map with
no address hint landed on them and was refused. A whole tick of hand investigation (worklog 284)
had missed it; the base was the one fact the empty-arena reasoning could not produce, and the
diagnostic just stated it.

Fix (D463): moved `MAPPING_BASE` to `0x0000_7400_0000_0000`, a clear terabyte above the data blocks.
Verified two ways - the reservation-failure line vanished, and PPSA04263 went 32 -> 10,123 calls
(`+10,091`), faulting far later at `image+0x2ba47bc`. Blast radius: PPSA25872 advanced 26,850 ->
39,929 (`+13,079`); PPSA21564 and PPSA02664 unchanged, no regression. `cargo clippy`/`test` clean
on mem, kernel and worker (the one clippy note is the pre-existing `enter` too_many_lines).

Surprises worth keeping: (1) the bug was structural, not title-specific - it only looked
PPSA04263-only because titles that reserve with a hint first never reach `MAPPING_BASE`. (2) With
failures now visible, several titles show a **non-fatal** conflict at `0x6b0000000000` - the guest's
own hint address being refused and its allocator falling back to the arena as `reserve_virtual_range`
is designed to do; the guest recovers, so it is not a wall, just newly legible. Worth understanding
later (why does the hint conflict?), not urgent. PPSA28061 shows a distinct `host-refused` at an
image address - its own issue.

Next oracle-free: chase PPSA04263's new wall `image+0x2ba47bc` (10k calls in now) and PPSA25872's
`0x7b594e` with the return column; both are freshly exposed and may be more local bugs.
