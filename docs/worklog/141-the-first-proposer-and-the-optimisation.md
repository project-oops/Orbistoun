# 2026-08-24 - The first proposer, and the optimisation that had to be reverted


**Done.** `crates/orbistoun-propose`, and the `Ask` seam in `orbistoun-llm` that makes it
testable. A round asks a model for candidate *words*, refuses what is wrong with the reply,
grows the grammar in memory, sweeps only the shapes the new words reach, and keeps what the
hash confirms. 21 tests, none needing a model or a network. Reasoning in D214.

**What it unblocks.** Nothing yet - deliberately. This is a library and no command calls
it. The `run-llm` entry point is next, and stub-semantics bisection after that.

**The test worth having.** A real grammar, a real hasher, a real sweep, a real hash, and
only the model faked. `Sema` is taken out of the vocabulary; a wrong word finds nothing;
the right word recovers `sceKernelCreateSema`; and the record is handed to `solve::verify`,
which is the function `audit` itself runs. The assertion is not "a name appeared" but
**"the audit re-derives it"** - which is the property the words-not-names route exists to
protect.

### Surprises

**An optimisation that would have destroyed the provenance record.** Sweeping only the
shapes that use the grown vocabulary is an 83x saving and is exact. The obvious next step -
restricting the vocabulary itself to only the *new* words - is another 200x, and I wrote
it before working out that it is wrong.

A `Generated` record is a pattern and an **index**, and an index is a position in a
mixed-radix number whose digits are the lengths of that pattern's word lists. Shorten the
list and the radix changes, so the recorded index names a different candidate in every
grammar anybody actually holds. `verify` would refuse names that are perfectly real, and it
would look like a naming bug rather than an arithmetic one.

Filtering *patterns* is safe for exactly the reason narrowing the slot is not: indices are
per-pattern, so dropping a shape cannot move a position inside the ones that remain.
Reverted, and pinned by a test that asserts the record still verifies against the grammar
*as it stands once the word is adopted*.

**A failing test found it, not review.** The assertion "a round sweeps under one per cent"
failed at 1.2%, which is what sent me looking - and the reason the arithmetic disagreed with
the code was that I had costed a design I had not built. Had the bound been loose enough to
pass, the narrowing would have looked like free money.

**A near miss worth an error.** Before building any of it I checked whether any pattern
actually uses the `learned` vocabulary. Two do. Had none, every word added would have
generated nothing - silently and permanently, each round reporting a clean miss
indistinguishable from an exhausted vocabulary. That case is now `Error::SlotUnused`.

**Two of this repository's own rules collide.** The prose guard forbids `\`
line-continuations in string literals, because `cargo fmt` bakes source indentation into the
rendered text, so it pushes writers to `concat!`. `format_args!` then refuses `{name}`
capture through a macro-expanded format string. **Every `concat!` format string here must
use positional arguments.** Cost one build; worth knowing before the next person hits it.

**The seam was missing and the crate could not be tested without it.** `Vocabulary::round`
first took `&Llm` - a struct that owns real backends and downloads gigabytes - so the entire
round was untestable. `orbistoun-llm` grew an `Ask` trait and the proposer takes `&dyn Ask`.
Principle 12's test applies exactly: it buys testability now rather than hypothetically.

### Also

`./orbistoun.sh check` failed once mid-session on five workspace-wide steps. Self-inflicted:
a non-compiling crate was added to the workspace members while the gate was running. Not a
real failure, but worth knowing that the gate has no protection against a concurrent edit -
and a second session was editing this repository throughout.

`orbistoun-llm` and `orbistoun-propose` are green on their own tests. The full gate has not
been re-run since these changes.

