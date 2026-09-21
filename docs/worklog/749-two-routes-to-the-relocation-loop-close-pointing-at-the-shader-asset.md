# 749. Two routes to the relocation loop close, pointing at the shader asset

**2026-09-21** — following 748's crisp "the guest's inlined loader relocated three of five", this tick
tried the two obvious ways to reach that loop and closed both. The negatives are worth as much as a hit
here: they rule out the watchpoint and the magic-scan, and they redirect at the one input the loader
reads that orbistoun actually controls — the shader asset the header is copied from.

## Route one: a watchpoint on the construction — blocked by non-deterministic allocation

`ORBISTOUN_HEAP_BASE` serves the heap from a region at a fixed address, which raised the hope of a
stable descriptor address and a watchpoint on its `+0x18` write. It does not deliver one: two runs under
`ORBISTOUN_HEAP_BASE=default` put the descriptor at `0x740007200600` and `0x7400069c0600` — same base,
different offset. The base is pinned; the allocation within it is not, because the number of allocations
before the descriptor varies run to run (concurrent execution, not the allocator). A watchpoint has to
name its address before the run, and the address is not knowable before the run, so this stays shut —
the same early-write-into-an-ASLR'd-object wall 748 named.

## Route two: find the loader by its magic — the magic is not in the code

The header carries ASCII `"1234"`, so the loader might test a dword against `0x31323334`. A scan of the
whole graphics-code region (`0x400000030000..0x400000048000`, every page) for that immediate in either
byte order found **nothing**. The reading that fits: the loader does not compare the magic as an
immediate, it **copies the header in from the shader asset** (magic and all), so there is no
`0x31323334` constant in the code to find. That is consistent with everything else — the header is
asset data the guest relocates in place, not a structure the code builds field by field.

## Where both negatives point

The loader relocates the header's relative fields by walking a table, and that table comes **from the
asset**, not from a constant in the code. On hardware the table names all five fields
(`+0x18,+0x20,+0x28,+0x30,+0x38`); the loader on orbistoun produced three. The asset is the title's own
file from `/app0`, identical on both machines — *unless orbistoun serves it wrong*. A shader blob read
short, mis-sized, or mis-aligned would hand the loader a truncated relocation table and exactly the
three-of-five it produced. That is the next check, and it is tractable without the loader's code: find
which `/app0` file the shader header is copied from, and confirm orbistoun serves its full length and
bytes (a length the guest reads from a `stat`/`fstat` or a header field is the usual culprit).

## Honest state

The wall is diagnosed to the byte: group 0's register data is present at `base+0xa8`, its pointer at
`+0x18` holds the raw `0xa8`, and the relocation that would fix it skipped `+0x18`/`+0x38`. What remains
is *why the loader's table was short*, and after this tick the leading answer is asset serving rather
than a guest-code bug or an API stub — a shift from "read the inaccessible loop" to "check a file
read", which orbistoun's own trace can show.

## Gate state

No code changed — a heap-base stability check, a code scan, and analysis. `./bin/orbistoun check` is
unchanged from 748 (green but for the same three generated-doc drifts from a prior session's uncommitted
`compat/PPSA02664-app0.toml` edit, not this tick). Identity scan clean. No commit.
