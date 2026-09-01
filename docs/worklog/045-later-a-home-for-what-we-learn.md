# 2026-08-20 (later) - A home for what we learn


D122. 470 tests. `crates/orbistoun-hle/data/knowledge/*.toml`, plus `learn` and `knows`.

Seeded with everything this session established about direct memory - the argument layout
measured from the guest, the ignored return value, the buffer-clearing requirement, the
rejected single-region map - and with the graphics names, honestly marked as names and
nothing more.

### Surprises

- **The duplication objection was right, and inverted the design.** A knowledge file looks
  like a third place holding name and arity. It is not: `guest_module!` was already
  holding knowledge inside code, and moving it out reduces the count. The right answer
  came from taking the objection seriously rather than defending the proposal.
- **A test caught the file contradicting itself** almost immediately: an entry marked
  "nothing beyond the name is established" had picked up a speculative edge case while I
  was demonstrating `learn`. Removed - a file that argues with itself is worse than a
  thinner one.
- **`main` had grown past the line limit** purely from adding verbs, which is the shape
  of a function that will keep growing. Split into configuration and dispatch.

### Outstanding

The provenance guard now fails on `crates/orbistoun-shader/tests/fixtures/unreached.bin` -
a generated fixture with a banned extension, belonging to the other session. Its exemption
list only covers `tests/fixtures/synthetic/` at the repository root, which predates
fixtures living inside crates. Left alone deliberately: it is their file, and how to
resolve it - rename, relocate, or widen the exemption - is a provenance judgement that
belongs to whoever owns it.

