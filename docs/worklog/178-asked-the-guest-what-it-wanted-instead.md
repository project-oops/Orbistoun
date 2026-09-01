# 2026-08-27 - Asked the guest what it wanted instead of guessing


D306 left five payloads dying at `0x1` having called nothing, each taking a pointer in `rdi`
and calling through it, with nothing here knowing the layout. Two new `EntryArgument`
diagnostics settle a lot of it in three boots (D308).

**Markers.** Every slot a different unmapped address, so the faulting address names the slot
it came from. All five payloads: **slot 0, offset 0**, to the byte. The first member is a
function pointer, called immediately. One boot for the whole structure rather than one per
candidate offset.

**Answering.** Every slot points at code that returns zero. The wall moved, and then moved
again once the instrument stopped conflating two kinds of field:

```
klogsrv   0x241b  ->  0x24a2  ->  0x2708
ftpsrv    0x241b  ->  0x7d42  ->  0x7fa8
elfldr    0x241b            ->  0x4a48
```

It ends at `0f 0b 0f 0b cc cc cc cc` - `ud2` twice with `int3` padding, a deliberate
compiler-emitted trap. **Not derailed**: the guest ran its own code, checked something and
rejected what it was handed, which is the D303 distinction landing on the good side.

### The instrument was wrong before the guest was

The first answering stub was three bytes in a page of zeros. The guest entered at `+0xa`,
ran off the end into `00 00` - `add [rax], al` - and faulted on a write. The report said the
guest wrote to a bad address: true, and entirely about my stub. A page of `ret` fixed it.

Then the second version handed every slot the same executable address, which could not tell
a field that is *called* from one that is *written through* - and the guest did both. Slot
zero now gets the returning page, every other slot its own writable one.

Third time this session a tool produced plausible output before the code under it did. Also
third time a `grep` over run output matched nothing and read as "no fault" - twice a pattern
that did not match, once a working directory that had reset so the binary never ran at all.
**Any summarising filter over run output needs its raw form checked once before it is
believed.**

### Where it goes next

The guest is rejecting *content* now, not presence, and no number of marker boots says what
a field must contain. The payload SDK documents its own handoff ABI, and principle 1 permits
reading another project's prose - recorded `published`, credited, written out rather than
pasted (they are GPL-3.0, this is not). Then promoted to `measured` when a marker run agrees.


### Two process failures that were structural, not careless

**Decision numbers raced.** The convention was read the highest and add one; two sessions in
one tree both read 312, both spent minutes writing, and both appended D313. Twice in an
afternoon. `./orbistoun.sh decide "<title>"` claims the number the instant it is chosen -
under a `mkdir` lock, before a word of the body exists - and the gate now refuses a
reservation nobody spent, because a claimed-and-abandoned number reads in the log like a
recorded decision.

The tool proved itself on first use: asked for a number, it returned **D319**, because the
other session had taken 317 and 318 while this work was in progress. The old convention would
have written 317 and collided a third time.

It also surfaced a collision already in the tree. `D308` was duplicated and **both were cited
from source** - seven times in `abi`/`gui`/`loader`, three in `names`/`propose`. The three
moved to D320, being the cheaper side and in crates this session was already in.

Worth recording: a blanket `sed` while renumbering overwrote one of the *other* session's
`(D308)` references in this file. Caught, restored. That is the hazard of two sessions in one
document, and it is an argument for narrow substitutions rather than for concentrating.

**The gate could not be run at all.** `check` compiles the whole workspace, and a crate
half-written by another session does not compile - so nothing could be verified, including
work that never touched it. An hour of finished work sat unverified behind it.

`check --only "<crates>"` narrows the cargo steps; the static gates stay whole-tree because
none of them compiles anything. The verdict never says `all checks passed` for a scoped run:
green is what a person scrolls to and reads as permission, and letting a subset borrow it
would build this log's recurring failure into the gate itself.

### The surprise

The first draft exempted `cargo fmt` from scoping, with a comment arguing that formatting
needs no compilation so another session's crate could not block it. **The very first scoped
run failed on exactly that.** The comment was more confident than the code, and was written
in the same change the run then contradicted.

The rule it missed is general: a step that cannot pass for reasons outside the scope makes the
scope useless. `cargo doc` had it too.

### The objection to generating patches was a misreading

`THE_LOOP.md` says a tool *"that produces plausible implementations with no verification step
makes the codebase worse rather than better"*, and that had been read here as *do not
generate implementations*. It does not say that. The operative clause is the middle one, and
a proposal somebody reads, gates and merges has a verification step - a stronger one than
most code in this tree got.

So a bundle carries proposals now. Each patch is a file in `patches/` with an entry beside it
saying what it changes, who or what wrote it, how the behaviour is known, and what it
assumes. `submit check` prints them **apart from the claims** and says nothing here checked
them, because a measurement is settled by re-deriving it and a patch by a person reading it -
one list would let a diff inherit the trust the measurements earned.

The constraint that actually binds is **provenance, not verification**. Principle 1 calls a
model in the loop a third route to the convergence problem, and generating an implementation
is where recall-dressed-as-reasoning is most likely and least visible. So a proposal carries
an oracle, and one resting on `assumed` is merged by somebody willing to say where the
behaviour came from, or not at all. A labelling requirement, not a prohibition - which is the
whole design of the `known_by` vocabulary.

Exercised end to end with a real diff: exported, carried, and reported back as
`[assumed, by a model]` with its assumption intact and a line saying it rests on nothing
better than a guess.

**Nothing generates these yet, and that is now a gap rather than a policy.**

