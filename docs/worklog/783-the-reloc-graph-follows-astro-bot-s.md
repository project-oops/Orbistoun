# 783. The reloc graph follows ASTRO BOT's indirection one level but the pointer-to-table is reached by computed addressing with no static ref; the invocation is untraceable statically, exhausting the retail RE approaches

**2026-09-21** — worklog 782 left ASTRO BOT (PPSA21564)'s arena-init reachable only "one indirection past
a lea/call scan" and named a reloc-graph query as the way past it. The query worked - it followed one
hop - and then hit the same 0-reference wall a level up. That is the honest end of what static RE reaches
here, and it is the same shape on all six titles.

## The reloc graph did follow the indirection

Parsing `DT_RELA` and querying for a relocation whose addend lands in the method table found exactly one:
`*(image+0x88dc070) = image+0x88dc148` (a `R_X86_64_RELATIVE`; the table starts at `0x88dc148`, arena-init
at `+0x10 = 0x88dc158`). So a global at `image+0x88dc070` holds a pointer to the table - the step a
`lea`/`call`-target scan cannot see, because the pointer is written by the loader, not by an instruction.
This is the tool 782 asked for, and it advanced the trace by one real level.

## But the next level is computed, not referenced

Following it up stops immediately:

- Nothing in the reloc graph points to `0x88dc070` (it is the chain root).
- A full-text code scan for references to `0x88dc070` found **0**.

So `0x88dc070` is reached neither by a relocated pointer nor by a `lea`/`mov [rip+..]`. The only way left
is **computed addressing**: code holding a base into the RELRO blob (`image+0x88dc000..`) and reaching
`0x88dc070`/`0x88dc158` as `base + offset` in a register. That base is established at runtime and cannot
be pinned by matching a fixed target address - a full-text scan for `0x88dc158`, `0x88dc148` and
`0x88dc070` all return 0 for exactly this reason. Static RE has followed the chain as far as fixed
addresses go.

## The honest state of the six

Every retail title's grind now ends at an **un-run startup construction whose invocation is behind
indirection static analysis cannot follow to a runnable caller**:

- GTA / ASTRO BOT (native): the construction never runs; its call site is computed/indirect, not a fixed
  reference (764-768, 777-783).
- Terminator (Il2CPP): strings and dispatch are metadata-table indexed, defeating string-ref RE (771-775).
- PPSA02664/03416/28061 (AGC): inline non-exports needing clean-room synthesis (769-770).

None is a loader bug (ASTRO BOT's relocations all apply, 782) or a missing named import. Crossing any of
them now needs a capability the loop does not have as a one-tick step: an **execution trace from entry**
that shows where startup diverges from calling these constructors, or a **differential against a
reference** that boots them - both sustained builds, not disassembly windows. The reloc-graph query is the
first of these tools and it is worth keeping; it just is not enough alone.

## Gate state

No code changed - a characterisation that shows the reloc-graph query following ASTRO BOT's indirection one
real level (to the pointer-to-table at `image+0x88dc070`) and then reaching computed addressing with no
static reference, and records that all six retail grinds now stand at the same untraceable-invocation
wall needing an execution trace or a reference diff. `./bin/orbistoun check` green, worklog index
regenerated, identity scan clean. No commit.
