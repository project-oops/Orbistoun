# orbistoun-propose

Where a proposal meets the thing that checks it.

**Models:** the pairing of a source of guesses with an oracle that can refuse them, and
the accounting of what each round tried, kept, and threw away.

**Deliberately fakes:** nothing. A round that could not ask anything is an error, never
an empty result - "nothing answered" and "nothing left to find" are different facts and
only one of them means stop.

## The rule this crate exists to enforce

```
something proposes  →  something else disposes  →  only what survived is kept
```

The first box is the least important one. What makes a proposer safe to build is the
second: an oracle that is cheap, mechanical, and cannot be talked into agreeing.
A proposer without one is a machine for generating plausible wrong answers, which
[CLAUDE.md](../../CLAUDE.md) principle 3 rules out in as many words.

So each proposer here is named after its **oracle**, not its source, and none is built
before its oracle exists.

| Proposer | Oracle | One query costs | A wrong proposal costs |
|---|---|---|---|
| `vocabulary` | the NID hash | a sweep, about a minute | **nothing** |
| stub semantics *(not built)* | the guest, re-run | one boot, one bit | a relaunch |
| an implementation *(no)* | - | - | unbounded |

## Why vocabulary is first

Its oracle is the best one in the project. A hash collision is **proof**, not a
judgement - no authority is consulted and none could help. So this is the one corner of
the codebase where a model that confidently invents things is *harmless*: an invented
word is discarded by exactly the same arithmetic that discards a carefully reasoned one,
and the bill is a sweep.

It is also the loop edge that compounds. A word learned from one title reaches hashes in
titles that have nothing else in common with it - one gave up `Sema`, which unblocked two
unrelated ones.

## Words, never names

The model is asked for **vocabulary** - `Sema`, `Attr`, `Prio` - and never for an
identifier. It is never shown a hash, never told which function is wanted, and never
given a mapping. A test holds the prompt to that.

This is not fastidiousness. A name confirmed through the word route is recorded
`generated` at a pattern and an index, so `orbistoun-cli audit` re-derives it by
evaluating that pattern - the same check every other generated name gets, with nothing
taken on trust. Had the model been asked for the name directly, the record could say only
*"something suggested this"*, nothing could re-derive it, and
[PROVENANCE.md](../../docs/PROVENANCE.md)'s answer to *"did you work these out
yourselves?"* would have a hole exactly where its foundation is.

The word route costs nothing to take and needs no new provenance category, so it is the
one taken.

Two guards sit behind the prompt, because a prompt is a request rather than a
constraint: a word must be a single capitalised alphanumeric token, and it must fit
inside a length that a whole identifier cannot. `sceKernelAllocateDirectMemory` fails the
first; `SceKernelAllocateDirectMemory` fails the second.

## What a round does

1. Ask for words, showing the convention by example and listing what is already known.
2. Read the reply - strictly if it is the JSON array that was asked for, loosely if not,
   **recording which**, because a model that never manages the strict shape is worth
   knowing about.
3. Sanitise: shape, length, novelty, duplicates, budget. Everything refused is reported
   with its reason.
4. Grow the grammar **in memory**, sweep, and keep what the hash confirms.

Nothing on disk changes. A round that finds nothing leaves no trace at all, which is what
makes a wrong proposal genuinely free - and it is why this crate needed no edit to
`orbistoun-names`: `Grammar.vocabulary` is already a public map, and the solver already
stamps its own derivations.

## What it does not do

**It writes nothing.** A round returns what it found and what it discarded; deciding what
to persist belongs to the caller, so the decision to change a tracked file stays in one
place instead of being buried inside a search.

## Status

`vocabulary` is built and unit-tested with no model and no network - the prompt, the
reply reader, and the sanitiser are all pure. The live path needs a configured backend;
see [orbistoun-llm](../orbistoun-llm/README.md), which configures itself on first use.

Stub semantics is next, and its specification is already written: the *Automated
stub-semantics search* entry in [docs/BACKLOG.md](../../docs/BACKLOG.md), including the
constraint that matters - each query costs a boot and returns one bit, so a prior is only
worth what it saves in queries.
