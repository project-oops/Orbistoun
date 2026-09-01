# D220 - The diagnostics share plumbing, and one of them is dyed banknotes


**decided** · 2026-08-24 · directed by the user, who asked for the whole toolkit rather than
one tool

Five measurement mechanisms were wanted. Building five more variables the way the first
three were built would have produced eight parsers, eight ways into the run report, and
eight paragraphs of documentation - which is the shape that drifts, and three separate
instances of exactly it were removed the same day (D213, D215, D217).

### The interface stays separate; only the plumbing is shared

`ORBISTOUN_STACK_FILL=5a` is easier to remember and to type than any grammar, so the
per-question variable stays. What moved into one place is the parsing, the import matching,
and the single line in the run conditions - `crates/orbistoun-worker/src/experiment.rs`.

The matching rule was the immediate win: it existed twice, and both copies had to say "by
name, or by any part of the label" so an **unnamed** function could be addressed by its
hash. That is not a convenience - the functions most worth experimenting on are precisely
the ones nothing has named (D213).

### One conditions field, and the one that was missing

`Conditions` carried `stack_fill` and `forced_write` as separate fields, and
`ORBISTOUN_DUMP` **was not recorded at all**. So a run under forced dumps was
indistinguishable from an ordinary one in the trace.

To be fair to it: dumps only observe, so a verdict stays comparable and this is not the
correctness bug the other two would have been. But "this run was instrumented" is worth
knowing when reading a report, and there was no way to know it. One `experiments` field
now, written from one `describe()`.

### The cheapest tool on the list, and why it was not the first one built

**Marked values.** Instead of writing plausible numbers into the memory-query structure,
each field carries a value that names itself - `0xAAAA…`, `0xBBBB…`, `0xCCCC…` in the bits
a real address cannot reach, with the true value in the low half so the walk still
advances. Whatever the guest does next says which field it read.

This is the standard black-box move and it needs *no machinery at all* - only different
bytes. It should have been suggested before the watchpoint and the field sweep, both of
which are more work and answer less. It had also already worked here **by accident**: the
guest's next query offset is the `end` value, which is how field 1 was known to be the walk
cursor. Nobody set out to learn that.

Run deliberately, it said two things:

- **Field 1 is the cursor**, confirmed by design rather than luck: the guest fed back
  `0xbbbb000200000000`.
- **The guest does not validate what it reads.** It accepted an obviously absurd physical
  address without complaint. So whatever makes it reject the map, it is not a sanity check
  on these values - which kills a family of theories.

And it named its own limit: fields 0 and 2 are never fed back or otherwise surfaced, so
this cannot see whether the guest reads them at all. That is exactly what the read
watchpoint is for, and the cheap tool answering what it can and naming what it cannot is
the ordering working.

### Heap poison, for the region the stack poison cannot reach

The host allocator returns uninitialised memory, which on a fresh page is almost always
zero - so a guest reading a field nobody filled in and a guest reading a deliberate zero
are indistinguishable on the heap. That is precisely the ambiguity D185 removed for the
stack, in the one place it could not reach. Zero is not a fill: it is what the host does
anyway, and rewriting every allocation to no effect would make the instrumented run slower
than the one it is compared against.

### And the honest framing of why any of this exists

Worth writing down because it is unusual. An emulator for this platform would normally
answer "what does this structure hold?" by reading an SDK header, another project's source,
or a disassembly of the vendor's own library. Principle 1 closes all three, deliberately,
and the cost is real: this problem is orders of magnitude slower here than it is elsewhere.

What is left is measurement. So the measuring tools are not a side quest - **they are the
method**, and each one converts a guess into an experiment.

