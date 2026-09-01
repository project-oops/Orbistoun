# 2026-08-24 - A diagnostics toolkit, and the registry that should have come first (D220, D221)


**Done.** Five diagnostics sharing one home, a new crate declaring every environment
variable, and `orbistoun-cli env` to list them. 214 tests across the seven crates touched.

### The cheapest tool was the one I suggested last

Asked how a *human* emulator developer would find the value the guest wants, the honest
answer is: **they would look it up** - an SDK header, another project's source, a
disassembly of the vendor library. Principle 1 closes all three, deliberately, and the cost
is real rather than notional: this problem is orders of magnitude slower here.

But not everything a human does is looking things up, and I had missed the standard
black-box move. **Marked values**: write numbers into the structure that name themselves,
and whatever the guest does next says which field it read. No watchpoint, no machinery -
only different bytes. I had proposed a read watchpoint and a field sweep first, both more
work and both answering less.

It also turned out to have already worked here **by accident**. The guest's next query
offset is the `end` value, which is how field 1 was known to be the walk cursor. Nobody set
out to learn that.

Run deliberately it said two things: field 1 is the cursor, **confirmed by design**; and
**the guest does not validate what it reads** - it accepted `0xbbbb000200000000` as a
physical address without complaint. So whatever makes it reject the map, it is not a sanity
check on these values. A family of theories gone.

And it named its own limit: fields 0 and 2 never surface, so this cannot see whether the
guest reads them. That is what the watchpoint is for. The cheap tool answering what it can
and naming what it cannot is the ordering working.

### Then the plumbing, then the thing that should have been built before the plumbing

Three diagnostics existed, each with its own parser, and one of them - `ORBISTOUN_DUMP` -
**was never recorded in the run conditions at all**, so a run under forced dumps was
indistinguishable from an ordinary one. (Fair to it: dumps only observe, so a verdict stays
comparable. Not the correctness bug the other two would have been.)

Unifying that was right. Building the typo check *before* the registry was not: it needed a
hand-written list of names to excuse, so it re-typed constants that already existed in
`orbistoun-paths` **and** a second copy of the five diagnostic names beside the ones
`from_env` used. Two fresh duplications, in the hour after removing three others.

Asked where the central list was, the answer was that there wasn't one. `orbistoun-env` is
it: nine variables, declared once, with a summary, a copyable example and the crate that
reads each. With a registry there is no exclusion list at all - "is this real" is a lookup.

### What the registry bought immediately

- The `docs/WORKFLOW.md` table of diagnostics was a hand-copy of three decision entries. It
  is a pointer at `orbistoun-cli env` now.
- `ORBISTOUN_STACK_FIL=5a` is reported as unrecognised instead of silently running an
  ordinary experiment.
- Settings and diagnostics are a **field**, which is what makes the `.env` question
  answerable: settings may come from a file, diagnostics may not. Not built - nothing needs
  it - but the rule is decided rather than deferred.

### The env-var choice itself, examined

The reason written down (D185: "a question is asked once, not configured") is true but
secondary. **The real reason is that the guest runs in a child process** and `Command::new`
inherits the environment, so a variable arrives free where a CLI flag would need threading
through the protocol. There are no explicit `.env()` calls anywhere in the tree - every
child inherits everything.

The costs are real and were being paid silently: invisible in `--help`, and a typo does
nothing. Both now addressed, the first by a command and the second by the registry.

### A flaky test I introduced, and a verification that could not see it

The final check turned up `18 passed; 1 failed` in `orbistoun-mem`, then passed six times
in a row. Two separate mistakes, and the second is the one worth keeping.

**The test.** The stack-span test added earlier chose `TEST_BASE + 0x300_0000` by reading
the file and picking a gap - and a test three functions further down was already using it.
These reserve **real host memory at fixed addresses** and tests in a binary run on parallel
threads, so the two raced. It failed about one run in ten: often enough to be real, rare
enough to be dismissed as flakiness. `docs/BACKLOG.md` had predicted exactly this and I
walked into it anyway.

Fixed by removing the choice rather than documenting it - `stack.rs` hands out bases from a
counter now, so no test picks an address. Twelve consecutive clean runs. The *cross-crate*
half of that backlog entry is untouched: a per-binary counter cannot see another binary.

**The verification.** I had reported "214 tests passed" while that test was failing,
because the command summed `N passed` with `awk` and never looked at whether anything
failed. A check that reports success because it was not examining the thing that went wrong
is the exact shape this session removed three instances of - in `found_by`, in the ceiling
early-return, and in the readable window - and I then did it to my own tooling within the
hour.

Worth stating as the rule it implies: **counting successes is not checking for failures**,
and `grep -c passed` is not a test run.

