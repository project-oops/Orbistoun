# 781. The ASTRO BOT init-table was a misread of the RELA relocation table; the arena-init is placed at a function pointer by a RELATIVE reloc in a RELRO segment, pointing the wall at reloc/RELRO handling

**2026-09-21** — worklogs 779 and 780 read the array at `image+0xee136f0` as a custom init-table and
could not find its walker. Parsing the dynamic section correctly shows why: that array **is the
relocation table**, and the arena-init's presence in it is a relocation, not a registry entry. This
corrects the reading and moves the wall onto something orbistoun owns - relocation and RELRO.

## The "init-table" is DT_RELA

The eboot's `DT_RELA` is at `image+0xee13650`, size `0xcc8c28`, entry size `0x18`. The arena-init
pointer `0x27cc20` sits at `image+0xee136f0`, which is `0xa0` into that table - i.e. the **`r_addend` of
RELA entry 6** (`0xee13650 + 6*0x18 + 0x10`). Read as a relocation, entry 6 is:

```text
r_offset = 0x88dc158
r_info   = 0x8            (R_X86_64_RELATIVE)
r_addend = 0x27cc20       (the arena-init)
```

So it is not a `{fn, arg, 8}` record; it is `*(0x88dc158) = base + 0x27cc20`. The `0x88dcxxx` values I
read as "args" are the `r_offset`s of adjacent RELATIVE relocs, and the `0x8`s are the `R_X86_64_RELATIVE`
type. 779's "custom init-table walked by something" is **withdrawn** - the walker was never found because
there is none; the loader consumes this table at load, which is exactly why the read-watchpoint on it
came back never-touched (780).

## What is really going on

The arena-init is invoked through a **relocated function pointer at `image+0x88dc158`** (with siblings
`0x88dc160/168/170` for `0x27cd30/cd80/cd90`, the arena's other methods - a **method table**). And that
pointer lives in a specific kind of segment:

- `ph[3]` is `PT_LOAD` `image+0x88dc000 .. 0x8e3c3e8`, flags `RW`.
- `ph[4]` is `GNU_RELRO` over the same range - **relocated at load, then made read-only**.

So `0x88dc158` is a RELRO function-pointer table entry: the loader must map `ph[3]`, apply the RELATIVE
relocations into it (writing `base + 0x27cc20` etc.), then protect it read-only, before anything calls
through it. A `PEEK` of `image+0x88dc150` at runtime reported it **outside every readable span** - which,
if real and not a `PEEK` span-list limitation, means the arena-init pointer is never placed because its
RELRO page is not mapped-and-relocated the way the segment asks.

That reframes ASTRO BOT's wall entirely: not "a function orbistoun's startup forgets to call", but
**"a RELRO relocation orbistoun may not apply, so a method-table pointer stays unpopulated"** - a loader
concern orbistoun owns end to end (D708/D709), and one that would explain a whole method table going
dark, not just one entry.

## Next

Two checks decide it. First, confirm whether `ph[3]`/RELRO is mapped and its RELATIVE relocs applied -
the loader's relocation tally and a direct read of `0x88dc158`'s runtime value (should be
`0x40000027cc20` if applied). Second, the caller: a scan for who references `0x88dc158` is running, which
names the site that calls through the method table. If the pointer is unpopulated, that call reads a bad
value; if it is populated but the caller never runs, the wall is elsewhere. Either way the question is now
about relocation into a RELRO segment, which is testable in orbistoun directly rather than by more title
disassembly.

## Gate state

No code changed - a correction that identifies 779/780's "init-table" as `DT_RELA`, reads the arena-init's
entry as a `R_X86_64_RELATIVE` reloc placing a function pointer at `image+0x88dc158` in a RELRO segment
(`ph[3]`/`ph[4]`), and repoints the wall at orbistoun's relocation/RELRO handling. `./bin/orbistoun check`
green, worklog index regenerated, identity scan clean. No commit.
