# orbistoun-propose

Where a proposal meets the thing that checks it.

It pairs a source of guesses with an oracle that can refuse them, and accounts for what each
round tried, kept and threw away. A round that could not ask anything is an error, never an
empty result: "nothing answered" and "nothing left to find" are different facts, and only one
of them means stop. It uses [orbistoun-llm](../orbistoun-llm/README.md) as the source and
[orbistoun-names](../orbistoun-names/) as the search.

## The rule this crate enforces

```text
something proposes  ->  something else disposes  ->  only what survived is kept
```

What makes a proposer safe is the second box: an oracle that is cheap, mechanical, and cannot
be talked into agreeing. A proposer without one generates plausible wrong answers, which
CLAUDE.md's *Honest failure* rules out. Each proposer is named after its oracle, not its
source, and none is built before its oracle exists.

| Proposer | Oracle | One query costs | A wrong proposal costs |
|---|---|---|---|
| `vocabulary` | the NID hash | a sweep, about a minute | nothing |
| stub semantics | the guest, re-run | one boot, one bit | a relaunch |
| an implementation | none | - | unbounded, so never built |

## Vocabulary

A hash match is proof, not a judgement, so this is the one place a model that confidently
invents things is harmless: an invented word is discarded by the same arithmetic that
discards a reasoned one. A word learned from one title also reaches hashes in unrelated
titles, so the search compounds.

The model is asked for words - `Sema`, `Attr`, `Prio` - never for an identifier. It is never
shown a hash, never told which function is wanted, and never given a mapping; a test holds the
prompt to that. A name confirmed through the word route is recorded `generated` at a pattern
and an index, so `orbistoun-cli audit` re-derives it like every other generated name. A name
the model suggested directly could be recorded only as "something suggested this", which
nothing could re-derive (see [docs/PROVENANCE.md](../../docs/PROVENANCE.md)).

Two guards sit behind the prompt, because a prompt is a request rather than a constraint: a
word is a single capitalised alphanumeric token, and it fits inside a length a whole
identifier cannot. `sceKernelAllocateDirectMemory` fails the first;
`SceKernelAllocateDirectMemory` fails the second.

## A round

1. Ask for words, showing the convention by example and listing what is already known.
2. Read the reply - strictly if it is the JSON array asked for, loosely if not, recording
   which.
3. Sanitise: shape, length, novelty, duplicates, budget. Every refusal is reported with its
   reason.
4. Grow the grammar in memory, sweep, and keep what the hash confirms.

A round changes nothing on disk, so a wrong proposal leaves no trace.

## Keeping what worked

`bank` keeps only the words a confirmed name was built from, in a file of this crate's own,
separate from `orbistoun-names/data/vendor.toml`, which the name search owns. A caller merges
the bank into a grammar in memory. Promoting a word into the shipped vocabulary is a
deliberate act with a diff, because it changes what every future search enumerates.

## Running it

Proposing is slow and opt-in, so it lives behind its own binary, `orbistoun-suggest`, and
nothing on the `./bin/orbistoun run` path waits on a model. Words it confirms are written to
`symbols/proposed-<slot>.txt`.

```bash
cargo run -p orbistoun-propose --release --bin orbistoun-suggest -- [rounds]
```

Before asking for words, check that words are what is short:

```bash
cargo test -p orbistoun-propose --release --test shapes -- --nocapture
```

reports whether the unnamed imports are short of vocabulary or of grammar shapes. When it is
shapes, more words buy nothing.

The prompt, the reply reader and the sanitiser are pure and unit-tested with no model and no
network.
