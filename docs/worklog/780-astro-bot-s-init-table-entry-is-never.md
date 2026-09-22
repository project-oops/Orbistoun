# 780. ASTRO BOT's init-table entry is never read at runtime, disproving the walker hypothesis; the arena-init's invocation is elusive, so the grind pauses like the others

**2026-09-21** — worklog 779 found ASTRO BOT (PPSA21564)'s arena-init registered as one entry in a
custom init-table at `image+0xee136f0` and expected a walker to iterate it. The runtime check refutes
that, and the honest result is that the invocation mechanism is not statically reachable - the same wall
GTA and Terminator's grinds hit.

## The table is never read

A static ref scan for the table region found **0** code references (worklog 779). The runtime test is
sharper: a read-watchpoint on the arena-init's table entry, `image+0xee136f0:rw`, came back **`never
touched`**. So nothing reads that entry before the fault - no walker calls through it. The static table
at `0xee136f0` is **not** how `image+0x27cc20` is invoked at runtime, and 779's "a walker iterates this
table" reading is withdrawn.

## Where that leaves the arena-init

The chain is solid down to here and then goes dark:

- `+0x7af792` reads `image+0xe553bd0` (null) -> faults (777).
- `image+0xe553bd0`'s only writer is `image+0x27cc20` (778), which never runs (its first write,
  `image+0xe553bb8`, is never touched, and it has **0 direct callers**, 779).
- Its address is in the file exactly once, in the `0xee136f0` table (779) - **which is never read**
  (this worklog).

So `0x27cc20` is invoked by neither a direct call nor that table nor a `DT_INIT_ARRAY` (there is none).
What remains are mechanisms static analysis does not follow cheaply: a table **copied/relocated** to
another address at load (the walker reads the copy), a pointer-to-table in a global set by relocation, or
a driver in a **module** rather than the eboot (only the eboot was scanned). Each is another indirection
hop, and ASTRO BOT - the most tractable retail title by construction - has now taken four ticks to reach
a link that needs a load-time-relocation trace or a module scan, not another disassembly window.

## The honest state, across all six

Every retail title's grind has now reached a wall of this shape - deep indirection or missing tooling
that a single loop tick cannot cross:

- PPSA04263 (GTA): un-run registration, input-manager-init unreached (764-768).
- PPSA25872 (Terminator): fatal pool alloc, Il2CPP metadata blocks static RE (771-775).
- PPSA02664/03416/28061: AGC inline non-exports, need clean-room synthesis (769-770).
- PPSA21564 (ASTRO BOT): un-run arena-init, invocation mechanism not statically reachable (777-780).

None is a missing named import to implement; all are sustained efforts. ASTRO BOT's is the closest to
mechanical - a relocation trace of what populates or copies the `0xee136f0` table would likely finish it -
but that is a tool to build (resolve the eboot's relocations to their runtime targets), not a loop step.

## Gate state

No code changed - a correction that withdraws 779's walker reading (the table is never read at runtime),
records the arena-init's invocation as reached only through load-time relocation or a module driver, and
notes all six retail grinds now stand at sustained-effort walls. `./bin/orbistoun check` green, worklog
index regenerated, identity scan clean. No commit.
