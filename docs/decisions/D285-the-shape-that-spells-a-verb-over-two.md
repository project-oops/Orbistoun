# D285 - The shape that spells a verb over two learned nouns


**decided** · 2026-08-26 · `sceKernelReserveVirtualRange`, and the 104 names behind it

The name that opened the `image+0xafc959` wall came out of the string harvester, not the
generator, and the reason is worth stating precisely: **every word it needs was already in
the vocabulary.** `Reserve` is a verb, `Kernel` a module, `Virtual` and `Range` both learned.
What did not exist was a pattern placing a verb before *two* learned nouns.

So this is not a vocabulary gap and no wordlist change would have helped. `tests/shapes.rs`
already measures exactly this, and ranks the shape **second** of every shape the corpus wants
and the grammar cannot spell:

```
145  +2577554%   prefix + learned + learned + learned + learned
105  +     99%   prefix + module + verb + learned + learned      <- this one
 97  +1175375%   prefix + module + verb + learned + learned + learned
```

**105 names for +99%** - roughly doubling the sweep. The one above it would unlock forty more
and costs twenty-five thousand times the whole space, which is the difference between a shape
worth adding and a shape that says the name has to come from somewhere else.

`tail` is omitted, following D261: `tail` holds the empty string, so a shape carrying it is a
strict superset of the same shape without, at about fifteen times the candidates. The name
that prompted this carries no suffix, so the cheap version is what the evidence asks for.

**It does not invalidate a single existing record, and the first draft of this entry said it
would.** A generated record is `{pattern, index}`, and re-derivation resolves the pattern *by
name* and asks it for its own index - so a new pattern adds a new enumeration and leaves every
other one exactly where it was. Measured rather than reasoned: thirty-three names were
unaccounted before this shape was added and the same thirty-three after it.

What *does* renumber is growing a **vocabulary** an existing pattern draws from, which is what
happened earlier the same day - `posix_vocabulary` splitting on underscores took it from 2,923
words to 3,741, and every index built from that list moved (D213). Worth separating, because
the two changes feel identical and only one carries a repair bill:

| change | effect on existing records |
|---|---|
| add a pattern | **none** - a new enumeration beside the others |
| grow a vocabulary | every index in every pattern using that list |

The cost a new pattern *does* carry is on `--repair` itself, which walks each pattern's whole
enumeration looking for a name: another shape is another space to search before giving up.

