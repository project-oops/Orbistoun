# 447. The largest record nobody read

**2026-09-08** - directed, continuing 446

## What was done

**`measure` is the most numerous record kind in every hardware report, and this project parsed
none of them.** 3,319 across the three reports, all landing in `Record::Other`, while the reader
printed a record count that was perfectly correct. Nothing was false; the largest thing in the
file was simply invisible (D605).

It parses now, graded like every other fact, and the report prints a per-section tally marking
every section it does **not** interpret:

```text
measurements  2588 in 19 section(s)
     16  120-measure/cache-topology  - carried, not interpreted
     18  136-kernel/handoff  - carried, not interpreted
   2443  140-oracle/kexport-table
```

Eighteen of nineteen still say *carried, not interpreted*. That is the honest state and it is
also the work list, ranked by how much evidence each line represents.

## The export table, and what it is worth

2,443 hashes against 2,405 addresses, from a live console's own kernel export table.

Reading it validates the hasher at a stroke: **2,141 of the 2,443 are named by the database this
project built out of its own vocabulary**, which is the D070 check repeated at a scale nothing
has offered before. Any other byte order matches none.

Thirty-eight addresses carry two hashes each - one function under two names - and where both
sides were already named the pattern was never compositional:

```text
0x8000006f0   getpeername   =  _getpeername
0x800012e00   inet_pton     =  __inet_pton
0x800105ae0   fileno        =  fileno_unlocked
0x8001683f0   _ZNSt6_WinitC1Ev  =  _ZNSt6_WinitC2Ev
```

## A name is a seed for the next name

That is a naming method the generator cannot express, because a whole finished name is not a word
in any of its lists. `crates/orbistoun-names/data/affixes.toml` holds the rules - prefixes,
suffixes, and the Itanium ABI's constructor and destructor numbering - and `Method::Affixed`
records which rule was applied to which held name, so a recheck is one rule against one string
rather than an index into a grammar (D606).

`_ZNSt14error_categoryD1Ev` is the one worth singling out: the export table said the hash at
`0x8000c4580` aliases the `D2Ev` spelling, the rules said the name was the `D1Ev` spelling, and
the hash agreed. Prediction and proof arrived independently.

## Feeding the table back in

`names --from-report` unions a report's unnamed exports into the target set - hashes to look for,
never names, since what comes back is proved by the hash whatever the report is. Deliberately
kept out of `symbols/wanted.txt`, which means *imports we cannot name*; a kernel export nothing
has ever imported is not one, and folding them together would move the number every report
quotes.

`bin/orbistoun names` picks reports up from `ORBISTOUN_PROBE_REPORTS` when it is set. Not a path
in the repository: the reports live in the sibling project, and writing that path down here would
be the cross-repository build dependency D207 exists to prevent.

**Eighty-nine names**, from every source at once:

| source | named |
|---|--:|
| published standard list | 29 |
| generated grammar | 35 |
| affixed | 19 |
| module strings, cross-module | 9 |

`sceKernelClose`, `sceKernelAllocateDirectMemory`, `sceKernelIsStack` and `atoll` were among them
- names the knowledge base has recorded behaviour for all along, that the symbol database did not
hold.

**And eighty-eight of the eighty-nine were never on any work list.** Only `_err` appears in
`wanted.txt` at all: the rest are hashes the console exports that no title in the corpus imports,
so no search this project could run would ever have reached them. That is the census D245
described, arriving.

`wanted.txt` moved 8,321 to 8,317. Three of the four are title imports the run named -
`sceKernelSyncOnAddressWait64`, `_err`, `sceVideoOutSubmitChangeBufferAttribute2` - and the
fourth left for an unrelated reason worth chasing rather than assuming: `0x23020f8e2805acae` is
`sceKernelAprWaitCommandBuffer`, which nothing named, but which orbistoun **declares** since 446.
The work list drops a hash when the emulator can name it, whether or not the database can.

## The bug the knowledge base caught

Those eighty-nine immediately broke `the_shipped_files_account_for_everything_they_claim` with
forty disagreements, and it was right to. A name settled by two sources in one run kept
whichever ran **first**, under a comment claiming sources run cheapest-first. They do not:
strings run before the published list, so every published C name that also appears in a module's
bytes was recorded as a static harvest - true, weaker, and untouchable by CI (D607).

| | before | after |
|---|--:|--:|
| named by the published list | 10 | **29** |
| named by cross-module strings | 60 | **9** |
| re-derived from this repository | 630 | **708** |

Seventy-eight records moved to the only tier CI can recheck, with no new search and no new
evidence. Six entries still disagreed afterwards and resolve the other way: the generative sweep
ran 3.9 billion candidates *with those hashes in its target set* and did not produce five of
them, so `generated` was a claim nothing could check while the names were absent from the
database. Their `found_by` fields are corrected.

## Surprises

- **The export table is not a naming input on its own.** It carries hashes and addresses and no
  names. What it gives is *which hashes exist* and *which are the same function* - and every name
  still comes from this project's own words agreeing with a hash.
- **Only 28 of the 2,443 exports were on `wanted.txt`.** Kernel exports and title imports are
  nearly disjoint sets: one is what the platform offers, the other is what a game asked for.
  Expecting a large overlap was wrong.
- A test pinned `measure` as a record kind deliberately left unparsed because no real output had
  ever carried one. Correct when written, overtaken by evidence, and the sort of claim that only
  ages badly if nothing re-reads it.
- Two of my own insertions detached a doc comment from its item again, and one duplicated a whole
  function. Same anchoring mistake as last session: anchor **after** the previous item's closing
  brace, never before the next doc comment.

## Two standing gate failures, and one of them was lying

`check` has had three failing steps for as long as this session has been running. Two are now
closed.

**`cargo doc`** was six public doc comments linking to private items, in five crates. Unlinked;
the prose still names them, rustdoc no longer tries to resolve them.

**`decisions`** was the interesting one. It printed thirteen numbers and said they *no longer
duplicate* - and every one of them still does, twice over. It was grepping `docs/DECISIONS.md`
for `^## D123`, the shape the log had when every entry lived in one file; the entries moved into
`docs/decisions/` and took their headings with them, so the pattern matched nothing and "listed
but not in the empty set" swept up the whole backlog. Following its instruction would have
emptied a ceiling file and hidden thirteen live duplicates behind a green step (D608).

A red step nobody has read the reasoning of is worth no more than a green one nobody has tested.
It reads the index table now, refuses to proceed on an empty read, and has been made to fail in
both directions.

**`prose` remains, and it is the real thing rather than a broken check** - but its ceiling has
stopped working. 47 files hold line-continued string literals, 166 lines in total, and
`docs/prose-continuation-backlog.txt` lists 18 of them. Every one of the other 29 reads to the
guard as *newly added*, so it fires on all of them every run and can no longer distinguish a new
offender from the pile.

This session added exactly one, in `orbistoun-turn`, and it is fixed. **The other 29 are not
mine to bless.** That file says of itself that it is a ceiling and not an allowance; regenerating
it would turn 166 lines of baked-in indentation into permission, and widening a guard to get a
green step is the one move that is never mine to make. It needs either the literals rewritten as
`concat!` - mechanical, 166 places, and it changes text a user sees - or a deliberate decision to
re-baseline. Flagged, not decided.

## The character tables regenerate byte-identically

`crates/orbistoun-libc/data/ctype.toml` - 272 entries each for `_Getpctype`, `_Getptolower` and
`_Getptoupper` - was generated from a capture called `injector-klog.obs.log`. The new pkg report
carries the same probe section, taken on different hardware-session on a different day.

```bash
orbistoun-gen ctype <the new report> --out /tmp/ctype-new.toml
diff crates/orbistoun-libc/data/ctype.toml /tmp/ctype-new.toml   # identical
```

Not one entry moved, and the generator's own spot-check refusal - twelve classifications read
through the *running* library rather than out of the table - passed on both. That is D181's rule
applied to hardware rather than to a build: a measurement rests on two independent observations
agreeing, and until today this table had one.

**The file is deliberately unchanged.** Regenerating it to cite the newer capture would produce
an identical table under a different provenance line, which is a diff that says something
happened when nothing did. The older capture still exists, in the sibling project's archive, so
the citation still resolves.

## Also

A raw `NUL` byte sat inside a byte-string literal in `orbistoun-kernel/src/lib.rs`, which makes
`grep` treat the whole 6,000-line file as binary and silently drop every match in it. Replaced
with a `\0` escape - identical bytes at compile time, and the file is text to every tool again.

The new report's clock measurements confirm D398 independently: `0x5f25_9b93` where the first run
said `0x5f25_9b8e`, cross-checked the same way (`0x4e8e` microseconds against `0x1e9d6e6` ticks
across one sleep, 1.593 GHz). `TSC_HZ` is deliberately left at the first reading - two
measurements five ticks apart do not say which is nearer, and moving it would change every
derived frame budget for no reason anyone could state.

## Next

- Eighteen measured sections still say *carried, not interpreted*. `120-measure/cache-topology`,
  `120-measure/clocks-advance` and `135-sysctl/names` are console ground truth for things
  orbistoun currently assumes.
- 215 kernel exports still unnamed after this pass. Extending the affix rules is the obvious
  next move and **it was tried and bought nothing**: thirty-eight further rules - `Ex`,
  `ForDriver`, `InternalHeap`, `Get`/`Set` and `Create`/`Destroy` inversions, more C++ ABI
  numbering - named zero. The method is at its limit for this seed set until the vocabulary
  grows some other way, which is a result rather than a gap.
- The eboot and pkg reports have not been fed in - only the payload one.
- The mapping sequence, still varying where the import count no longer does.
- The clean title's wall.
