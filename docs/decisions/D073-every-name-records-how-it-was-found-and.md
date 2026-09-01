# D073 - Every name records how it was found, and the record is checked

**decided** · 2026-08-19 · prompted by the user

Names are the one artefact here somebody could reasonably ask hard questions about. A
hash-to-name mapping is valuable, public databases of them exist, and "we worked it out
ourselves" is a claim like any other unless something can check it. The concern that
prompted this was exact: *commit this in six months with hundreds of names and no way to
show they were not lifted*.

**The argument is reproducibility, not record-keeping.** A name this repository's own
generator can produce is self-evidently derivable from this repository; a name it cannot
produce is the one that needs explaining. That turns a question about intent into a
question about arithmetic, which anyone can settle without trusting anyone.

**Four kinds, and the line that matters is not where it first appears to be.**

> **`observed` was split in two by D213**, which found that 137 of the 154 names carrying
> it had never run anything. The current vocabulary is `published-standard`, `generated`,
> `static`, `runtime`, `supplied`, on two axes. The rest of this entry is the reasoning
> that still holds; the table below is the shape it had at the time.

| Kind | Ours? | Mechanically checkable? |
|------|-------|-------------------------|
| `published-standard` | yes | yes - membership of a list in this repo |
| `generated` | yes | yes - one array lookup |
| `observed` | **yes** | no |
| `supplied` | **no** | no |

`observed` is the one worth being careful about. A name learned by debugging a title, by
a conformance probe, or by a test that pinned it down is **entirely ours** - this project
watching its own experiments is as clean-room as generating a candidate. It is simply not
reproducible by re-running an index, so it is recorded distinctly rather than dressed up
as generated. It will likely become the commonest kind once guests run far enough to be
instructive.

`supplied` never verifies, is listed on its own, and says outright that this repository
did not derive the name. That variant existing is what makes the rest of the file
trustworthy.

**The claim is re-run, not read.** Verifying a `generated` record evaluates the named
pattern at the recorded index and compares. A forged record therefore fails exactly as
loudly as a missing one - demonstrated by injecting a fabricated name with a plausible
pattern and index, which the audit rejected. Cost is an array lookup per name, so
thousands audit in under a millisecond, which is what makes it a gate on every commit
rather than something somebody meant to do before publishing.

Runs in `./orbistoun.sh check` and in a dedicated CI job over every database in
`symbols/`. Exits non-zero on anything unaccounted for.

**Derivations are written at the moment of discovery**, by the code doing the work, with
the date. A provenance record assembled afterwards is a reconstruction; one written at
the time is evidence.

**And the unnamed are persisted too.** `--wanted` writes the hashes still unresolved.
Without it every run rediscovers the same work list and forgets it, and that list is
exactly what the next round of vocabulary work is aimed at.

Full argument, including its honest limits, in [PROVENANCE.md](../PROVENANCE.md).

