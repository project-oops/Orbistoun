# D051 - Fixtures are generated, never extracted

**decided** · 2026-08-19

Nothing derived from `titles/` is ever committed - not bytes, not a header, not a
trimmed copy, not a hex dump pasted into a test. If a fixture needs to resemble a real
container, the generator builds it from the *structure* that was observed, never from
the file.

**Why this needs stating rather than assuming.** `tests/fixtures/synthetic/` is
deliberately **exempt** from the provenance guard, because those files are committed
on purpose and may legitimately carry banned extensions. That exemption is correct and
it also makes the guard blind in precisely the place the temptation lands: mid parser
work, "just save the first 4 KB of this real module as a fixture" is an easy and
entirely natural thing to do, and nothing would catch it.

So this rule is unenforced by tooling and held by discipline alone - which is exactly
why it is written down rather than left implicit. D014 bans firmware and dumps; it did
not say anything about carving fixtures out of them.

Practical consequence: observations from real material are recorded as **facts in the
decision log** (D049's magic bytes, offsets, and `e_type` values) and reproduced by
code. The bytes themselves stay outside the repository.

