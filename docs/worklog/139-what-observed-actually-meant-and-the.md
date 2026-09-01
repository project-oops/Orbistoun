# 2026-08-24 - What `observed` actually meant, and the ceiling that was not one (D213)


**Done.** The name-provenance vocabulary split by what was observed, three harvesters
added, one checked and refused, and `symbols/unaccounted-ceiling.txt` emptied.

### The finding that started it

`Method::Observed` documented itself as *"a name worked out by watching something run"*.
**137 of the 154 names carrying it had never run anything** - they were literal strings
read out of a file at rest. The value had been written for one mechanism and was being used
by another.

Raised from outside: a reading of the term rather than a bug report, and the code agreed
with the reading the moment anyone counted.

### It was hiding a capability, not just a label

The interesting part is not that the name was wrong. It is that **the static tier was
checkable the whole time and nothing checked it**, because a bucket holding both "read out
of a file" and "concluded from a running guest" has no check that applies to both, so it
got none and both were reported as *documented, not verified*.

A string in a module is there deterministically. `audit --verify-harvest` re-reads every
module a static record names and confirms it contains the string: **200 of 200 hold.** CI
cannot run it - there is no corpus and never will be - and that is the point of the tier
existing separately.

### The ceiling was measuring the wrong thing entirely

`symbols/unaccounted-ceiling.txt` held **202 names**, and its own header explained them as
vendor symbols the grammar could not spell: a real gap, slowly shrinking.

Wrong, and wrong in a way nobody would have caught by reading it. A generated record is a
pattern plus an **index**, and an index is a position in a mixed-radix enumeration over the
vocabularies - so adding a word renumbers every candidate built from it. Adding words *is
the loop* (D195). Every successful search was quietly knocking already-recorded names off
the verified list onto a file whose whole rule is that it may only shrink.

`audit --repair` hashes the stale names and hands them to the ordinary pattern search as
targets, so one sweep repairs all of them regardless of how many went stale. **All 202 came
back**, plus 20 more that a single 37-word learning pass had knocked off while this was
being written. Names re-derived from this repository alone went 290 -> 512 in that one
pass, **without a single new name being found**.

The number had been sitting there for weeks being read as a fact about the vocabulary.

### And the guards - two holes, and the second is the interesting one

`audit` returned early on "every name is accounted for", *before* the ceiling comparison.
So the half of the ceiling's rule that says an entry which stopped applying must leave was
unenforceable in the only state that can trigger it. An empty unaccounted set with a stale
202-name ceiling passed, silently.

Then the knowledge base. `FunctionKnowledge::found_by` is a second, hand-written copy of a
fact the symbol database already audits, and **eleven entries contradicted it** - six libc
names sold short as `observed`, three C++ ABI names claiming a list they were never in, and
`sceKernelWrite` marked **`supplied`**: that we did not derive a name the generator
produces. Every correction came from the audited record or from an `audit --deep` walk;
none by hand.

**A gate was running on that the whole time and passing.**
`the_shipped_files_account_for_everything_they_claim` asserts `provenance_faults` is empty
on every `cargo test`. It was green on all eleven, because the function checked `known_by`
and its citation and had never looked at `found_by`. Not a missing gate - a gate reporting
success on a field it was not examining.

**Fourth instance of the shape** (D191, D199, the ceiling above). Promoted from coincidence
to rule: *a guard is not finished until somebody has made it fail.*
`crates/orbistoun-cli/tests/provenance.rs` holds the new ones to it - four of its six tests
assert a failure.

Left open and named: an **implemented** function's name never enters the unnamed set, so no
derivation is recorded for it: 57 of the 95 declared functions have none. The 38 that do
have one only by luck of ordering - a search confirmed them before anything implemented
them. A hole in the audit's coverage, not only in this field.

### The corpus is one search now

`names` takes a directory: every module read, unnamed imports unioned, one sweep. It was a
loop over four hand-written globs in `orbistoun.sh` running the full 2.6-billion-candidate
sweep **once per module** - forty-two times, for a search whose cost is one hash-set lookup
per candidate whatever the target set holds. **An hour became 85 seconds.**

The globs also omitted `.sprx`. **Eleven modules had never been searched once**, and
nothing said so, because a glob matching nothing is not an error. One of the first names the
fixed walk found came out of `fakelib/libSceAmpr.sprx`.

### What the new harvesters actually found

- **`cross-module`** - 18 names on the first run, and the shape of them is the argument for
  the mechanism: `memcpy_s`, `strcpy_s`, `sprintf_s`, `vsnprintf_s` - the C11 Annex K
  family, sitting as strings in the vendor C library module, naming imports of titles that
  never mention them. A module-at-a-time search could not have found those however many
  titles arrived.
- **`argument-dump`** - identifier-shaped runs in what a guest passed to an import, read
  out of memory as it ran. The first mechanised runtime harvester; the seventeen existing
  runtime names were all worked out by hand. **It found nothing**: 20 candidates from 113
  captures across 6 previous runs, none matching. The source is the limit, not the
  mechanism - dumps are forced per-import, so nearly all of them are scalars with no text.
  Left in the loop and printing its zero with the reason, because a source that silently
  contributes nothing looks exactly like a working one. Widening what gets dumped is a
  change to the run, not to the search.
- **`probe-transcript`** - reachable via `--words-from probe` and **never exercised**,
  because obSCEne has not exported a name list yet.
- **Symbol-table residue was checked and does not exist here.** It would have been the
  cheapest source of all. No module in the corpus has a `.symtab`; five have a `.shstrtab`
  and nothing else. Not implemented, and written into PROVENANCE.md so the next person does
  not spend an afternoon establishing it.

Total: 669 -> **737 names**, nothing unaccounted for.

### Everything then ran again, and found nothing - which is the useful part

A second full pass over all 53 modules with every mechanism live: **2,609,721,996 generated
candidates named 0. 550,450 corpus strings named 0. The runtime harvester named 0. The
published list named 0.** 3,698 imports remain unnamed.

**Do not re-run this expecting a different answer.** The mechanisms are at their plateau
against the current inputs, and the next name has to come from a word the vocabulary does
not have or a source not yet read - not from another sweep. Worth knowing before somebody
spends an afternoon on it.

Incidentally it also confirmed the causal chain behind the ceiling: no names found means no
words learned, which means no indices moved, which means `--repair` had nothing to do. The
202 entries really were a by-product of success, not of failure.

### Two smaller things worth keeping

`crates/orbistoun-names/src/strings.rs` opened by claiming symbol tables as a source. They
are not one here, and that sentence had been sitting in the file long enough to be believed.

The `prose` guard catches a **new** line-continued literal - the cause. It cannot see one a
formatter already collapsed - the consequence - and one of those was printing a garbled
message out of the name search on every run. Three fixed; six more are on the backlog with
the reason they were not fixed in passing.

### Not touched, deliberately

`Oracle` in `orbistoun-hle` grades *behavioural* claims and carries the same confusion in
`Measured`, which means "on real hardware" and has no slot for measured-somewhere-else.
obSCEne mirrors that vocabulary in its own grading, so changing it one-sided is the drift
shared grading exists to prevent. It goes on the bridge instead.

