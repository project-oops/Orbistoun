# 775. The Terminator pool-name watchpoint was never touched and the string identity was conflated; the grind pauses for Il2Cpp metadata tooling and the loop rotates

**2026-09-21** — worklog 774 named a read-watchpoint on the "Hostname Lookup Memory Pool" bytes as the
tool that would reach the pool creation. It ran, and the honest outcome is that the lead was built on a
conflation. Recording it so the next attempt does not re-run the same watchpoint or re-inherit the wrong
string.

## What the watchpoint said

`ORBISTOUN_WATCHPOINT` takes `w`, `rw` or `x`, not a bare `r` (the first attempt was refused with exactly
that message). Re-run as `0x400001e54278:rw`, the pool-name bytes came back **`never touched`** across
the whole run to the fault. So nothing reads that string before the abort - the pool is not identified by
reading its name there.

## The lead was a conflation

Two different "Hostname" strings exist, and worklog 774 merged them:

- `image+0x1e5dccc` holds `"%s\0Hostname Look…"` - this is the string the abort's formatter loads
  (`lea rdi, [0x1e5dccc]` at `image+0x1755487`), and its `"%s"` is a **format**, not the error text.
- `image+0x1e54278` holds `"Hostname Lookup Memory Pool"` - a *separate* literal found by a binary
  string search, never shown to be on the fault path.

774 read the second as the pool behind the fault. The watchpoint's "never touched" plus this adjacency
mean that identification is **unconfirmed**: the fault is a fatal allocation failure (773 established
that solidly), but *which* pool, and its name, are not pinned. The memory-allocation log fragments on the
stack are real; the specific pool name is not.

## Why the grind pauses here

Four ticks (771-774) localized PPSA25872's wall to a fatal memory-pool allocation whose root is upstream
of the fault's call stack, then hit three walls in a row: the stack does not contain the creation
(worklog 774), Il2CPP's metadata string tables defeat string-reference navigation (774), and a
read-watchpoint only helps if the name's bytes are read - which they are not if the pool holds the name
by pointer (this worklog). Reaching the creation now needs a **parser for the title's Il2CPP metadata**
(to resolve the string table and the pool registry to real addresses), which is a build in its own right,
not a loop-tick step. That is the right next tool but the wrong size for this cadence, so the grind is
**paused with its state recorded**, not abandoned - 771-775 hold the full trail.

The loop rotates to the one retail title it has not yet examined this pass, PPSA21564, rather than sink a
fifth tick into a lead that needs tooling first.

## Gate state

No code changed - a correction that retracts 774's pool-name identification (the watchpoint disproved it
and the string was conflated), fixes the watchpoint type (`rw`, not `r`), and pauses the PPSA25872 grind
behind the Il2CPP-metadata tooling it now needs. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
