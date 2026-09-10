# 484. The corpus has names

**2026-09-09** - directed, continuing 483

Asked for a generated page per title with the missing metadata and placeholders for what has not
been captured. Built.

## What was already there

`compat/<title>.toml`, one per title, split into what orbistoun sets and what it got - the second
half transcribed from a trace and never typed, because a hand-written grade drifts. `compat
markdown` already rendered a ranked table, and already had screenshot plumbing nobody had filled.

What it could not say was what a title **is**. Every row was an identifier.

## Derived rather than typed

A `template.md` per title would put the drift back one level up. So metadata comes from
`sce_sys/param.json` - the title's own statement of itself - and the pages are regenerated from the
records every time:

| identifier | title |
|---|---|
| PPSA02664 | Alex Kidd in Miracle World DX |
| PPSA03416 | Summer Sports Games |
| PPSA04263 | Grand Theft Auto V |
| PPSA21564 | ASTRO BOT |
| PPSA25872 | Terminator 2D: NO FATE |
| PPSA28061 | Earthion |

The table links each name to its page; the name sits beside the id rather than replacing it,
because the id is what traces, records and cross-project requests all key on (D660).

## Absent things are shown, and the two kinds told apart

Menu and gameplay rows always render, with a shared `no-capture.svg` where nothing has been taken -
a gap reads as broken, a dropped row reads as though nobody asked. And **"ships no `param.json`"**
is said differently from **"the field is missing"**: a homebrew payload has no metadata file at
all, and four rows of *not recorded* would send somebody after a parser bug that is not there.

## Surprises

- **The first version threw away every name it read.** The metadata refresh sat above an early
  return taken whenever a run is not better than the record - and every title in the corpus is at
  its best already, so it would have skipped all of them. Found by opening a generated page and
  seeing it empty, which is the argument for generating them where somebody will look.
- **A test caught the table change correctly** - `md.contains("far 📷")` broke when the title cell
  became a link. Updated to assert the whole cell, since a bare `far` also matches the link target
  and would have passed whatever the mark did.
- **obSCEne ranks third**, above Terminator, GTA V and ASTRO BOT. It has no display name because it
  ships no `param.json`, which briefly read as it being missing from the table entirely.

## Next

- Captures need the GPU path; the slots are ready and will stay placeholders until a frame renders.
- Prosperous `c8b5` still unblocks both unmeasurable walls.
