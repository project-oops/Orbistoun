# The dispatch loop was not where the cost was


D110 emits the dispatch loop even for single-block shaders, and deferred collapsing it
"until there is something to measure". Nothing measured it, which is how a deferral becomes
permanent - so it is measured now.

An empty module is 591 words. One instruction adds 8. The fixed preamble - types, two
register files, two storage buffers - is seventy times what an instruction costs, so
collapsing the loop would save a fraction of a percent and buy a second emission path.

The decision stands, on arithmetic instead of judgement. 248 tests green.

### Surprises

- **The reason written in the entry had expired.** It kept the loop because a second path
  would be the under-tested one, single-block shaders being all the tests there were. Eight
  named tests build multi-block shaders now. The conclusion survived; its argument did not,
  and it would have kept being repeated.
- **The measurement pointed somewhere else entirely.** The question was framed as "is the
  loop worth collapsing" and the numbers answer "the loop is not the cost" - the module's
  fixed preamble is. Nobody was asking about the preamble.
- **A size assertion would not have caught the thing it was for.** Collapsing the loop
  barely moves a 591-word total, so the test looks for the loop and the switch directly
  instead.

