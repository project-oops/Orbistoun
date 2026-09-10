# D660 - A page per title

**Status:** assumed
**Date:** 2026-09-09

## What existed, and what it could not say

`compat/<title>.toml` has held one record per title for months, split deliberately: what orbistoun
**sets** is written by a person with a reason, and what it **got** is transcribed from a trace and
never typed - because *"a hand-written grade drifts the moment somebody is optimistic, and nothing
can check it afterwards"*. `compat markdown` renders them into one ranked table.

What it could not say is what a title **is**. Every row was an identifier, so the corpus read as
`PPSA02664`, `PPSA25872`, `PPSA28061` - and nobody working on it could tell you which was which.

## Derived, because the alternative is the thing the split exists to prevent

The obvious answer is a `template.md` and a hand-written page per title. That puts the drift back
one level up: stale the first time somebody records a run and forgets to edit prose, and
unfalsifiable in exactly the way the status half is built to avoid.

So the metadata is read from `sce_sys/param.json`, which the title ships and which names itself, and
the pages are **generated** from the records every time. Nothing is typed and there is nothing to
go stale. The localised name is preferred over the top-level one, because that is where a title
puts the name a person sees; the default language names itself, so nothing here picks a locale.

The corpus, immediately:

| identifier | title |
|---|---|
| PPSA02664 | Alex Kidd in Miracle World DX |
| PPSA03416 | Summer Sports Games |
| PPSA04263 | Grand Theft Auto V |
| PPSA21564 | ASTRO BOT |
| PPSA25872 | Terminator 2D: NO FATE |
| PPSA28061 | Earthion |

The name sits **beside** the identifier rather than replacing it: the id is what every other
artefact keys on - traces, records, requests to sibling projects - and a table showing only names
could not be read against any of them.

## Absent things are shown, and the two kinds of absent are told apart

A page renders every row whether or not there is anything in it, because a page that omits what it
lacks cannot be told from one that was never finished. Two distinctions are kept:

- **"ships no `param.json`"** is not **"the field is missing"**. A homebrew payload has no metadata
  file at all, and four rows of *not recorded* reads as a failure to parse one - sending somebody
  after a bug that is not there. Those pages say the file is absent, once.
- A capture that has not been taken gets a **placeholder image**, not a gap and not a dropped row.
  A gap reads as broken; a dropped row reads as though the question were never asked. One SVG,
  drawn once, shared by every title without that capture.

Menu and gameplay both always appear. A title that reaches a menu and one that has never drawn a
pixel produce the same shape of page, so the difference between them is the image rather than the
layout.

## Status is `assumed`

Nothing here is measured - it is a presentation decision about material already measured
elsewhere, and its shape was asked for rather than derived. The one substantive claim, that
deriving beats typing, is the same argument `compat/README.md` already makes about grades.

## A bug this found in itself

The first version refreshed the metadata and then returned early when the run was not better than
the record - so the name it had just read was thrown away on every title except one that happened
to improve. Every title in the corpus is at its best already, so **every** record would have been
skipped. Caught by looking at a generated page and finding it empty, which is the argument for
generating them where somebody will look.
