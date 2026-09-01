# The vocabulary regrew, and the only alarm was a clock


`./orbistoun.sh check` had stopped finishing. Not failing - finishing. The last log from it
showed six `orbistoun-propose` tests each past sixty seconds and no verdict, which reads as
a slow machine rather than a broken tree, and that reading is why it went unexamined.

Two things were true at once and neither announced itself.

**A leftover process.** The killed gate's test binary was still alive, and had accumulated
**four and a half CPU-hours**. Every timing taken while it ran was competing with it, which
is worth stating because two of the measurements that shaped the next hour were taken in
that state. Killing orphans is now the first thing to do after any interrupted gate.

**A curated list had regrown sixty-seven times over.** `learned` is 177 words. It was
11,842, harvested from guest module strings by a filter that accepts any capitalised
alphanumeric run - which describes Itanium C++ mangling exactly as well as it describes a
vendor word:

```
Agent5pause   Agent6enable   Agent2gc   Document9terminate   Layer18accumulated
```

6,451 of 11,845. And `learned` appears **twice** in two shapes, so the cost squares: 165
million candidates per shape at 177 words, 757 billion at 11,842. A vocabulary round went
from ~350 million candidates to 1.5 trillion. The test that had been running nine and a
half minutes in release was hashing its way through that.

Restoring the list took the same test to **20.23 seconds**, passing.

**The audit then did exactly what it is for.** Shrinking the list moved every generated
index again; `--repair` re-derived 26 of the 33 stale records and left 7 standing:
`sceAjmBatch*` and `sceLibcMspace*`. Those are D259's seven, and D259 names the cause -
`Batch` and `Mspace` are in no list. They had been restored into the working tree's regrown
list and nowhere else, so restoring the committed 175 dropped them a second time. Adding
them back gives **177**, which is the number D262 quotes, and the audit went to *every name
is accounted for*. The list is self-checking to that precision, which is a good sign about
the machinery even though it took two passes to notice.

**The tests no longer pay the loop's quadratic cost.** They drop the shapes that use
`learned` twice - safe where dropping a *word* would not be, because an index is a position
inside one pattern's own radix (D214) - and the records they produce are now verified
against the **complete** shape set, which is a stronger assertion than the one it replaced.
The suite went from over fifteen minutes unfinished to **76 seconds**.

### The surprise worth keeping

The thing that mattered was a *number in a data file*, and nothing in the tree reported it.
Not the audit, which passed. Not the tests, which passed when they finished. Not CI, which
would have timed out and been read as flaky. The only instrument that noticed a four
thousand-fold cost increase was wall-clock time on a test about something else.

D262 costed these shapes and called the reduction "its most valuable consequence". D259
repaired what the reduction stranded. Neither left anything that would stop the next
harvest putting the words back - and the next harvest did, silently, and undid both.

### And the filter that let it happen

`is_word` now refuses a digit followed by two lowercase letters. **Two rather than one, and
`Audio3d` is why** - the obvious rule rejects it, and it is a module word behind 168
confirmed names. Requiring two refuses 6,253 of the 6,451 fragments and **none** of the 468
words the shipped grammar holds. The 198 survivors (`Attrib1f`, `Buffer4k2k`) are still
fragments; nothing here can tell them from `Audio3d`, and being wrong in that direction is
the cheap one - a fragment that survives costs a sweep, a word that does not costs a name.

**This is necessary and not sufficient.** 5,394 of the 11,845 would still get through, which
is thirty times the curated 177. What holds a list at a size is a gate on *sweep cost*, not
a spelling rule, and that is a mechanism nobody has agreed to yet - flagged rather than
built.

### Four dependencies the crate split left behind

`cargo-machete` named them: `orbistoun-propose` still declared `orbistoun-env`,
`orbistoun-hle` and `orbistoun-report` after all three moved to `orbistoun-turn`, and
`orbistoun-turn` declared `orbistoun-core` it never used. Advisory findings, but the split
exists so that `cargo tree -p orbistoun-turn` shows **no path to a model runtime** - and
principle 12 puts that on crate boundaries rather than on anybody remembering. A declared
dependency is a boundary with a hole in it, waiting for one `use`. Removed; the isolation
still holds and machete is clean.

