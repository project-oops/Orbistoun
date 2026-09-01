# D214 - Proposals are paired with an oracle; the first one asks for words, not names


**decided** · 2026-08-24 · directed by the user, who chose the order of work

`crates/orbistoun-propose`. The callers that let something other than a person turn
[THE_LOOP.md](../THE_LOOP.md)'s steps 17 and 18. `orbistoun-llm` (D212) supplies proposals
and knows nothing about this project; `orbistoun-names` supplies the oracle and knows
nothing about models; this crate is where they meet, which is why neither of them has to.

### Named after the oracle, not the source

```
something proposes  ->  something else disposes  ->  only what survived is kept
```

The first box is the least important. What makes a proposer safe is the second: an oracle
that is cheap, mechanical, and cannot be talked into agreeing. Without one it is a machine
for generating plausible wrong answers, which principle 3 forbids in as many words. So
each proposer is named after its oracle, and none is built before its oracle exists.

| Proposer | Oracle | One query | A wrong proposal |
|---|---|---|---|
| `vocabulary` | the NID hash | a sweep, under a minute | **nothing** |
| stub semantics *(next)* | the guest, re-run | one boot, one bit | a relaunch |
| an implementation | none | - | unbounded, so no |

**Vocabulary is first because its oracle is the best in the project.** A hash collision is
proof rather than judgement - nothing is consulted and nothing could help - so this is the
one corner of the codebase where a model that confidently invents things is *harmless*: an
invented word is discarded by exactly the arithmetic that discards a carefully reasoned
one. It is also the loop edge that already compounds (step 7 of THE_LOOP).

### Words, never names

The model is asked for vocabulary - `Sema`, `Attr`, `Prio` - and never for an identifier.
It is never shown a hash, never told which function is wanted, never given a mapping.

**This is the whole provenance argument and not a preference.** A name confirmed through
the word route is recorded `generated` at a pattern and an index, so `audit` re-derives it
by evaluating that pattern - the same check every other generated name gets, with nothing
taken on trust. Ask for the name directly and the record could say only "something
suggested this": nothing could re-derive it, and PROVENANCE.md's answer to *"did you work
these out yourselves?"* would have a hole exactly where its foundation is. It would also
need a new provenance category, days after D213 finished removing the one that blurred
two claims together.

The word route costs nothing to take, so it is the one taken.

**A prompt is a request, not a constraint**, so two guards sit behind it: a word must be a
single capitalised alphanumeric token, and short enough that an identifier cannot fit.
`sceKernelAllocateDirectMemory` fails the first; `SceKernelAllocateDirectMemory` fails the
second. Both are tested, as is the absence of any hash from the prompt.

### The seam that made it testable

`Vocabulary::round` first took `&Llm` - a struct that owns real backends and downloads
gigabytes - which made the entire round untestable. `orbistoun-llm` grew an `Ask` trait,
`Llm` implements it, and the proposer takes `&dyn Ask`.

Principle 12's own test for whether a seam is premature: *"if it pays off only
hypothetically it is speculation; if it buys testability now, it is structural."* It buys
it now, and every proposer written later gets it free.

What that unlocked is the test worth having: a real grammar, a real hasher, a real sweep,
a real hash, and only the model faked. `Sema` is taken **out** of the vocabulary; a wrong
word finds nothing; the right word recovers `sceKernelCreateSema`; and the record is then
handed to `solve::verify` - the function `audit` itself runs. The assertion is not "a name
appeared" but **"the audit re-derives it"**.

### The sweep is a delta, and stops one step short of where it could

Only the shapes that reference the grown vocabulary are swept. Every other shape generates
exactly what it generated before, and the caller's ordinary sweep has covered it: 31
million candidates instead of 2.6 billion, an **83x** saving, exact rather than
approximate.

**The next narrowing was written, measured, and reverted, and that is the entry.**
Restricting the slot itself to only the new words takes a round to about 150,000
candidates - another 200x. It also destroys the record.

An index is a position in a mixed-radix number whose digits are the lengths of that
pattern's word lists. Shorten the list and the radix changes, so the recorded index names
a *different candidate* in every grammar anybody actually holds. `verify` would then refuse
names that are perfectly real, and the failure would look like a naming bug rather than an
arithmetic one. Filtering *patterns* is safe for precisely the reason narrowing the slot is
not: indices are per-pattern, so dropping a shape cannot move a position inside the ones
that remain.

Eighty-three times cheaper with the record intact beats seventeen hundred times cheaper
with the record meaningless (principle 11). A test pins it - `verify` must still accept the
record against the grammar *as it stands once the word is adopted* - so that this cannot be
re-optimised by somebody reading the sweep figures and not this entry.

**Found by a test, not by review.** The assertion that a round sweeps under one per cent
failed at 1.2%, which is what sent me looking - and the reason the arithmetic disagreed
with the code was that I had costed the design I had not built.

### Two things checked rather than assumed

**Does any pattern use the `learned` vocabulary?** If none did, every word added would
generate nothing - silently, and forever, with each round reporting a clean miss
indistinguishable from an exhausted vocabulary. Two shapes use it. That near miss is now
an error: `Error::SlotUnused` refuses a slot no shape references.

**A `concat!` format string cannot capture from scope.** The prose guard forbids `\`
line-continuations because `cargo fmt` bakes source indentation into the rendered text, so
it pushes writers to `concat!` - and `format_args!` then refuses `{name}` capture through a
macro expansion. Every `concat!` format string in this repository must use positional
arguments. Cost one build.

### What is deliberately not here

**Nothing is written.** A round returns what it found and what it discarded; persisting is
the caller's decision, so changing a tracked file stays in one place rather than being
buried inside a search.

**And nothing calls this yet.** A `run-llm` entry point that turns the loop with a model
and falls back to a person is the next piece; this crate is its foundation, not its
delivery.

