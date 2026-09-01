# D213 - Harvesting is categorised by what it observed, and the tiers are checked


**decided** · 2026-08-24 · raised by review of what `observed` actually meant

`Method::Observed` said, in its own doc comment, *"a name worked out by watching something
run"*. **137 of the 154 names carrying it had never run anything.** They were literal
strings read out of a file at rest by the static harvester (D193), and the value covering
them had been written for a different mechanism entirely.

That is not a wording problem, because the two are different claims with different checks:

- A string in a module is there deterministically. Re-read the file and it is there again.
  Anyone holding that title can verify the record.
- A conclusion drawn from a running guest is reproducible only by running it again, and a
  guest is not obliged to reach the same place twice.

One bucket holding both had to describe both as *documented, not verified*, which
understates the first considerably. **The static tier was checkable the whole time and
nothing checked it.**

### The vocabulary

`Method::Observed` is replaced by two variants, each carrying a closed subtype naming the
mechanism, so records can be counted rather than read:

| Record | `by` | Evidence | Reproducible |
|---|---|---|---|
| `published-standard` | - | derived | from this repository |
| `generated` | - | derived | from this repository |
| `static` | `module-strings`, `cross-module` | static | + the module |
| `runtime` | `call-trace`, `argument-dump`, `probe-transcript` | runtime | + a run, or hardware |
| `supplied` | - | external | only from its source |

Two axes rather than one, because they answer different questions - *what kind of material
proposed this* and *what would somebody else need to do it again* - and they are not the
same partition: a probe transcript is runtime evidence but sits in a tier above a local
run, because the console is not something CI can be handed.

Both are **derived from the variant, never stored**, so a record cannot claim a tier its
own method does not support.

**One vocabulary, not two.** `Oracle` in `orbistoun-hle` grades *behavioural* claims and
carries the same shape of confusion in `Measured` (which means "on real hardware" and has
no slot for measured-somewhere-else). It is deliberately not touched here: obSCEne mirrors
it in its own grading, and changing a shared vocabulary one-sided is the drift shared
grading exists to prevent. It goes on the bridge.

### Confirmation was never what varied

Worth writing down because it reframes the whole question. Every mechanism here is
confirmed the same way: hash the candidate, compare against what a real module declares it
needs. **The hash is the oracle, and it is arithmetic.** A record therefore describes only
how a candidate was *proposed* - which is why "is this static or runtime?" is a question
about the proposal and not about the proof, and why a string harvested from a title is
exactly as *proved* as a generated one while being differently *reproducible*.

### Three harvesters added, one checked and refused

- **`cross-module`.** Strings from the whole corpus are now pooled and tried against every
  module's imports, not just the module they came from. The vendor C library module carries
  its own function names, so this named the entire C11 Annex K family - `memcpy_s`,
  `strcpy_s`, `sprintf_s` and thirteen more - and used them to explain imports of titles
  that never mention them. A module-at-a-time search structurally could not find those,
  however many titles arrived. **18 names on the first run.**
- **`argument-dump`.** Identifier-shaped runs in the bytes a guest passed to an import,
  captured by the dispatch path while it ran (D198). Post-relocation memory, so it can hold
  text no module contains as a literal. The first mechanised runtime harvester; the
  existing runtime names were all worked out by hand.

  **It has found nothing, and it runs anyway.** Over the whole corpus: 20 candidates from
  113 captures across 6 previous runs, none matching a wanted hash. The limit is the source,
  not the mechanism - dumps are *forced per-import* rather than captured broadly, so nearly
  all of them are scalars with no bytes at all. Two choices follow from that. It stays in
  `./orbistoun.sh names` rather than behind an occasional flag, and it prints its zero with
  the reason attached, because a source that quietly contributes nothing is indistinguishable
  from one that is working. The way to make it productive is to widen what gets dumped, which
  is a change to the run and not to the search.
- **`probe-transcript`.** `--words-from probe` replaces `--words-from observed`, and now
  records what it is: ours, and the one tier of our own work no machine here can reproduce.
- **Symbol-table residue was checked and does not exist.** It would have been the cheapest
  source on the list. Every module in the local corpus: no `.symtab`, no `.strtab`, five
  with a `.shstrtab` and nothing else. Not implemented, and written down so it is not
  re-established by somebody else's afternoon.

### The corpus is one search, not fifty-three

`names` takes a directory. Every module beneath it is read, their unnamed imports are
unioned, and one sweep answers all of them.

This was a loop in `orbistoun.sh` over four hand-written globs, running the full
2.6-billion-candidate sweep **once per module** - forty-two times, for a search whose cost
does not depend on how many hashes it is looking for, because each candidate is one
hash-set lookup either way. A corpus search now takes 85 seconds rather than an hour.

The globs also omitted `.sprx` entirely. **Eleven modules had never been searched once**,
and nothing said so, because a glob matching nothing is not an error. The walk is in the
tool now, where it is one rule with a test - the same fix `is_version_script` needed for
symbol maps (D191).

### The record shape that fights the loop it belongs to

A generated record is a pattern plus an index. The index is what makes checking it a
microsecond rather than a sweep, and it is also a position in a mixed-radix enumeration
over the vocabularies - so **adding one word renumbers every candidate built from it**.

Adding words is the loop (D195). So every successful search quietly knocked
already-recorded names off the verified list and onto `symbols/unaccounted-ceiling.txt`, a
file whose entire rule is that it may only shrink. It had reached **202 entries**, and its
own header described them as vendor names the grammar could not spell.

**That description was wrong.** The grammar could spell every one of them. `audit --repair`
hashes the stale names, hands them to the ordinary pattern search as targets, and
re-derives all of them in one sweep - the cost is one pass regardless of how many records
went stale. All 202 came back, plus 20 more that a single 37-word learning pass had knocked
off the same way. Names re-derived from this repository alone went from 290 to 512 in
that one pass, without a single new name being found. **The ceiling is empty.**

The date is never rewritten. A record says when a name was first worked out; only the
coordinates moved.

### The second vocabulary, in a file nothing was checking

`FunctionKnowledge::found_by` in the knowledge base is the same claim about the same name
as the symbol database's derivation - hand-written, and free text. It drifted, as a second
copy does. **Eleven entries contradicted the audited record**, in both directions:

- six libc names recorded as `observed` that the published-standard list produces,
- three C++ ABI names recorded as `published-standard` that no shipped list contains,
- and `sceKernelWrite` recorded as **`supplied`** - the one label that says this project did
  not derive a name - when the generator produces it. Confirmed by `audit --deep`, which
  walks the whole space and re-derived it, as it did for the two other names whose correct
  value was not otherwise knowable.

None was corrected by hand: every replacement came from the audited record or from a `--deep`
walk. `provenance_faults` now checks the label is current vocabulary *and* that it agrees
with the symbol database wherever the database has a record.

The duplication itself remains, and it is worth naming why: **an implemented function's name
never enters the unnamed set**, so the search records no derivation for it: 57 of the 95
declared functions have none, and the 38 that do have one only because a search happened to
confirm them before anything implemented them. That is a gap in the audit's coverage, not
just in this field. Backlogged, with the cheapest route - sweep the declared names once and
report derivability, which is the only claim the audit ever makes.

### And the guards - one that could not have caught it, one that was not looking

`audit` returned early on "every name is accounted for" - *before* the ceiling check. So
the half of the ceiling rule that says an entry which stopped applying must leave was
unenforceable in the only state that can trigger it: an empty unaccounted set with a stale
ceiling passed silently. Fixed by running the check on both paths.

The knowledge base is the sharper case, because **a gate was running the whole time and
passing.** `the_shipped_files_account_for_everything_they_claim` asserts `provenance_faults`
is empty on every `cargo test`, and it was green on all eleven contradictions - because the
function checked `known_by` and its citation and had never looked at `found_by` at all. Not
a missing gate. A gate reporting success on a field it was not examining.

Fourth instance of the shape in this repository (D191, D199, and the ceiling above). It is a
pattern rather than four accidents, and the rule that falls out of it is:

> **A guard is not finished until somebody has made it fail.**

`crates/orbistoun-cli/tests/provenance.rs` now does exactly that for each new one - a static
record naming a module that lacks the string, an absent module that must report *unchecked*
rather than pass, a stale ceiling against an empty unaccounted set, and a generated record
at the wrong index. Four of the six tests assert a failure; the passing direction is the
cheap half.

### Consequences accepted

- **The serialised format changed**, and the committed database was rewritten. Greenfield,
  so it was edited rather than migrated (principle 10). The rewrite is not taken on trust:
  `audit --verify-harvest` re-reads every module a static record names and confirms it
  contains the string, and all 200 hold.
- **`--verify-harvest` is off by default.** It reads and scans the whole corpus, which is
  far too slow for a gate that runs on every commit, and a machine with no titles would
  report only "unchecked". `./orbistoun.sh names` passes it, because that is the command
  that already has the corpus open.

