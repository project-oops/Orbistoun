# 412. The whole corpus was twelve days stale

**2026-09-04** - directed

## What was done

Re-ran the six titles whose records predated D563's `answered` measurement, honestly - `learned.toml`
set aside - and at each record's own time limit, so the comparison is against like.

**Five of the six had improved enormously, and nobody had looked.**

| Title | Recorded | Now | Reach |
|---|--:|--:|---|
| PPSA03416-app0 | 23 imports | **188** (174 answered) | entered → **flipped** |
| PPSA25872-app0 | 14 | **129** (118 answered) | entered |
| PPSA04263-app0 | 4 | **70** (63 answered) | entered |
| PPSA21564-app0 | 13 | **57** (56 answered) | entered |
| obscene | 178 | **193** (180 answered) | entered → **flipped** |
| PPSA28061-app0 | 47 | 25 - **not recorded** | entered |

**Three titles now reach `flipped`** where this morning none did, and the ladder rung added in
D558 turns out to describe a third of the corpus rather than one title.

Those records were dated 2026-08-23. This is D555's finding - *the wall was real and the record
was twelve days stale* - repeated across the whole corpus rather than one entry. The work had
happened; nothing had re-measured it.

## PPSA28061 went backwards, and it is not today's doing

Twenty-five imports against a recorded forty-seven, deterministically - two runs identical. It was
worth ruling out, and the trace does: **it calls none of the functions implemented today**. No
`setjmp`, no `setcancelstate`, no `sigprocmask`, and it touches neither the event queues nor
video-out at all.

Where it stops is `libSceUlt` - the user-level threading library, five functions, all
unimplemented - and then the Agc calls. `sceAgcDriverRegisterDefaultOwner` and `sceAgcCreateShader`
both answer the placeholder and the guest **calls `abort`**, after which it hands that placeholder
to `malloc` as a size.

So the old entry is a claim from a build twelve days ago that the current one does not reproduce,
and the cause is somewhere in those twelve days rather than in today.

## The gap that leaves, which is the mirror of D563's

**A compatibility record can only move up.** `beats` records an improvement and refuses everything
else, so a title that gets *worse* keeps its old entry and the record quietly overstates what the
current build does. Nothing notices, because nothing is looking for it - the same shape as the
`standing` blindness fixed this morning, pointing the other way.

PPSA28061's row now says `2026-08-23` next to six rows saying `2026-09-04`, which is the only
signal there is, and it is one a person has to spot. Recorded here rather than fixed, because a
"best ever" record and a "what it does now" record are two different things and choosing between
them is a design decision, not a bug fix.

## Surprise

**PPSA25872 reaches 129 imports at 3% standing** - 11.5 million calls, and ninety-seven percent of
them on placeholders. It is the clearest case yet of why `standing` still earns its place beside
`answered`: the function count says the interface is mostly there, and the call share says the run
is nearly all spin.
