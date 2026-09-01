# D330 - The harvest could undo the curation, and a one-line list read as seventy-six words


**decided** · 2026-08-27 · found by checking whether `sweep` was safe to run

`./orbistoun.sh sweep` starts by calling `names`, which harvests strings from guest modules
and merges them into `learned`. That is how the list reached 11,842 words the first time
(D320). The mangling filter added since catches 6,253 of the 6,451 fragments - and **5,592
would still get through**, taking one `learned x2` shape from 169 million candidates to 169
billion.

So the project's own corpus command was a landmine: running it would have undone the day's
work and made the search unrunnable again, silently, exactly as before.

**The filter was never going to be enough**, because the thing that matters is not whether a
word looks like a fragment - it is what the vocabulary costs. `learn_words` now costs a
vocabulary round before writing, and refuses a set of words that would push it past a ceiling,
saying the numbers and what the choice is: curate the words, or drop a shape that uses the
slot twice.

`Learned::Refused` is a variant rather than a `None` for the reason the whole file keeps
running into: a silent refusal and "nothing was new" are indistinguishable, and the second is
what a clean run looks like.

### The bug the ceiling found on its way in

The first ceiling refused the *shipped* grammar. Costing it gave 27.9 billion where hand
arithmetic said 367 million - 76 times too much, and the factor is the answer:

`find_list` located a vocabulary list by searching for `\n]`. A one-line list has none:

```toml
prefix = ["sce"]
```

so the span ran past it into the **next** list and swallowed it whole. `current_words(grammar,
"prefix")` answered 76 words for a list holding one - the `prefix` entry plus all 75 of
`module`.

Live, and invisible: every existing caller passed multi-line lists, so nothing had ever asked
the question this got wrong. It only surfaced because a new caller needed the size of a
one-line list, and because the number it produced was checked against arithmetic rather than
believed.

**Worth the entry for that reason.** The gate found a latent bug in the code it was measuring,
before it had refused anything real - and if the ceiling had been picked to accommodate 27.9
billion instead, the bug would still be there and the ceiling would be eleven times too
generous to stop anything.


