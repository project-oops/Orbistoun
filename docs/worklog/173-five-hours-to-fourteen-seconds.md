# Five hours to fourteen seconds


`audit --repair` was given thirty-three stale records and did not finish in five hours. The
first suspicion was `solve::derive`, which walks `0..pattern.len()` comparing candidates -
a linear scan of a space this project measures at 1.52 trillion. That was worth fixing and
**was not the bottleneck**.

`Pattern::name_at` reads an index as a mixed-radix number, so recovering the index from a name
is the same arithmetic backwards. `Pattern::index_of` does that, with backtracking where a slot
has several words that could start the remainder - a greedy longest-first match reports a name
the pattern *can* spell as one it cannot, which the test asserts against `sceKernelCreateEx`.

**Then the actual cause.** `repair_generated_records` never called `derive` at all: it called
`solve_patterns` - the **full generative sweep**, hashing every candidate across every pattern,
looking for NIDs it already had the names for. A repair knows the name. It never needed to
search.

```
33 records, five hours, unfinished
33 records, 14.137s, every name is accounted for
```

And it removed a hazard the hash route had and guarded against by hand: a target set holds
hashes, the first candidate hashing to one is not necessarily the name being repaired, and
rewriting a record from a collision would forge coordinates **using the tool built to prevent
forged records**. Searching for the name cannot collide.

`--threads` went with it. It was documented as "threads for a `--repair` sweep", and there is
no sweep - arithmetic per name does not spread across cores.

### What it cost to find

The measurement was wrong twice before it was right. Five hours of runtime read as thoroughness
rather than as a bug, because a repair that has not finished looks exactly like a repair being
careful - and the gate stays red either way with nothing saying which. Then `index_of` was
built against the wrong suspect, on the strength of reading `derive` and assuming the repair
used it.

Reading the caller would have taken a minute.

