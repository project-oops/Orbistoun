# D180 - Behavioural provenance, because abstinence is not enforceable


**decided** · 2026-08-21

`found_by` recorded how a **name** was arrived at, and CI re-derives every committed name
from this repository's own inputs (D119). Nothing did the same for **behaviour** - an
arity, a return kind, what happens at an edge - and those are the facts that change what
the emulator does. Seventy-two entries recorded behaviour; none said how any of it was
known.

### The route principle 1 does not name

Principle 1 forbids code written while reading vendor binaries, and forbids lifting from
other projects. Both describe a *reading step* that can be pointed at.

Facts now arrive by way of a model that has read the public internet, including the other
projects in this space and the databases they ship. "This is what the function does" can be
**recalled and then dressed as reasoning**, which is the same convergence with no reading
step to point at. The stated goal is an unattended loop of exactly this shape, so it stops
being a hypothetical the moment it runs.

Abstinence cannot be the mechanism: it is unenforceable and, worse, unprovable. The
defensible claim was never "I never saw it" - it is **"here is how each fact was derived"**,
which happens to answer three separate worries with one field. A licence question (did this
come from someone else's source?), a quality one (is this reasoned or generated?), and an
operational one (which of our facts are actually guesses?).

### The vocabulary is the enforcement, not the field

`known_by` takes `published`, `measured`, `guest-observed`, or `assumed`. **Every value is
falsifiable, and there is deliberately none meaning "I already knew it."** Recording a fact
therefore means committing to a checkable claim about its source, which is a different act
from absorbing one silently - and a wrong claim is auditable later against hardware, where
a silent absorption never was.

`published` and `measured` claim outside support, so they must cite where. An uncheckable
claim of external support is worth strictly *less* than an honest `assumed`, because it
reads as evidence.

A fifth value for "by analogy to a published interface" was considered and rejected. It is
how most of libkernel is actually understood, but it would have become the comfortable
default that absorbs precisely the recalled-knowledge cases this exists to catch. Analogies
go in `assumptions`, where hardware can settle them.

### Per-claim, because the mixed entry is the normal one

One provenance per function would have to round a mixed entry up or down, and rounding up is
how a guess becomes a fact. `assumptions` lists what `known_by` does not cover. `snprintf_s`
is the worked example: the base interface is ISO C 7.21.6.5 and citable, while the
bounds-checked variant is Annex K - *optional*, implementations differ, and both its
truncation return and whether it terminates the buffer are unverified.

### Refused, not defaulted

`learn` rejects an entry that does not account for itself. Every available default is a lie:
`assumed` understates work really done, anything stronger overstates it. Refusing costs one
retry and is the only option that cannot record something untrue.

`assumed` is not a failure state and 10 of 72 entries are there. **The open-question count is
expected to rise** as more is written down - an assumption only appears once somebody notices
it - and to fall as hardware answers them. A number that only ever falls is measuring candour
rather than knowledge, which is why it is reported next to the breakdown rather than alone.

### What the backfill showed

| resting on | entries |
|---|---|
| `published` | 42 - libc, genuinely: ISO C, POSIX, and the Itanium C++ ABI all specify these by name |
| `guest-observed` | 20 - vendor interfaces a real title established by experiment |
| `assumed` | 10 - entries whose stated purpose is read off the name and nothing else |

60 open questions, each one a thing a conformance probe on real hardware could settle. That
list is the probe worklist, ranked by how often a guest actually touches it.

