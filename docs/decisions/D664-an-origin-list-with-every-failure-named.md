# D664 - An origin list, with every failure named

**Status:** measured
**Date:** 2026-09-10

## One origin per source was two wrong answers

A corpus source named exactly one place to get its assets: a release, or a sibling checkout. Both
are right some of the time and neither is right always. A checkout is the fast path for whoever
has one and absent for everyone else; a release is slower and always there. With one field, a
manifest either broke for people without the sibling or ignored it for people with it.

A source now carries `sources` - origins tried in order, first one that answers wins.

## Bare strings, classified here

The manifest lists origins as plain strings so a person can paste a release URL beside a relative
path without also learning a keyword for each. Reading them is this code's job:

```rust
Origin::classify("https://…")            // Url
Origin::classify("../obscene/build")     // Path
Origin::classify(r"D:\builds\probe")     // Path
```

That last one is why this is a tested function rather than an inline guess. A Windows path
contains a colon and `C:` is not a scheme, so the test is `://` and not `:`.

## Every failure is kept, and that is the point

`Attempt` carries what answered **and** what did not, with each reason. A fallback that reports
only its success hides the fact that the fast path is broken - the sibling checkout could have
been missing for a month while the release quietly served every fetch, and the manifest would look
healthy. All of them failing is an error naming every attempt, so the next person knows whether to
fix a path or a URL.

## Pure, so the ordering is testable without a network

`first_that_answers` takes the fetching as a closure. The ordering, the reporting and the give-up
condition are decided in a function that touches neither filesystem nor network, and the effectful
part is supplied by the caller - the shape principle 8 asks for, and the reason three tests cover
it exhaustively in microseconds.

Written test-first. The failures were `cannot find function first_that_answers`, `cannot find type
Attempt`, `unresolved import Origin` - which is what a failing test should say before there is
anything to pass.

## What is wired and what is not

The manifest carries the field, `corpus list` shows every source and its target, and the
decision layer is complete and tested. **The fetch path still uses `repo`/`tag`/`path`**: walking
an origin list at fetch time is the next step, and wiring it half-way would have left a manifest
that describes something the code does not do.
