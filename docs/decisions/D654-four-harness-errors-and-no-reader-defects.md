# D654 - Four harness errors and no reader defects

**Status:** measured
**Date:** 2026-09-09

## The differential's real answer

D653 reported 29 modules, 29 disagreeing, in four classes, and said the most likely defect was in
this repository. **All four classes were the harness.** With them fixed: **29 modules, 0
disagreements.** The two readers agree on every field the test compares.

That claim in D653 is withdrawn here rather than edited there - the record of a wrong reading is
worth keeping, and this is what the log is for.

## The four, in the order they were found

**1. `Elf::parse` takes an ELF, not a container.** Orbistoun's `Container::parse` unwraps on the
way in; SELFish's does not. The first run reported all 29 as `not an ELF` because the two were
reading different files.

**2. `Wrapper::is_wrapped` misses the previous-generation magic.** `is_either_generation` is the
test. Seven modules survived the first fix because of it.

**3. Do not unwrap at all - splice.** The inner ELF of a signed container is a *view*: its program
headers describe segments whose payloads live in the container's entry list. Handing SELFish that
view means handing it a file it has not got the bytes for, which is the 22 refusals D653 called
"the biggest class and the one I would look at first". `selfish_container::Container::to_elf`
splices the payloads back in. **The answer was not to unwrap more carefully but to not unwrap at
all**, and SELFish supplied it.

**4. Two field pairs that share a name and not a meaning.**

- `vendor_tables` says *the values orbistoun holds came from vendor tags*. `Info::table` says
  *which convention the file follows*, and under `Current` the standard tables come from standard
  tags - which is exactly when orbistoun answers `false`. Compared as `is_some()`, every module in
  the corpus disagreed and none of them actually did. Against `Some(Legacy)` they all agree.
- `dynamic_bytes` finds `PT_DYNAMIC`; `tables()` finds resolvable vendor tables. A plain
  freestanding ELF has the first and not the second, so the one non-vendor module in the corpus
  read as orbistoun over-reporting.

And the units, which SELFish named: `Elf::tables` measures from the slice it returns, orbistoun
from the image. Adding the holding segment's `vaddr` - read from the program headers, not
hardcoded from the one module where the constant was noticed - makes every address match exactly.

## What this says about differentials

**A differential measures the harness until somebody proves it doesn't.** Four rounds, four
errors, each of which produced a confident and completely wrong report - including one that named
a specific defect in a specific function of a sibling project, and one that named a defect here.
Every round *looked* like a finding. The failure mode is not subtle in hindsight and was invisible
at the time, because a disagreement between two readers is exactly what the tool exists to
produce: there is no shape a wrong answer has that a right one does not.

The same rule as a guard nobody has watched reject something, one level up. A comparison is not
comparing anything until somebody has checked what it is comparing, and the check is not "does it
report differences" - it is "do the two sides mean the same thing by each field".

All four are written into the test's own comments, beside the line each one broke, rather than
only in this record and the resolution. The next person to wire these two crates together meets
them where they will cost a round.

## The result is worth more than a list of differences

Two readers, built from the same facts, drifted apart across two repositories, agreeing on every
table field of 29 real modules. That is a real statement about both - and it is worth exactly as
much as the harness is trustworthy, which is why the harness's own history is in the file.

The symbol level - import triples and the relocation census - is reachable for the first time now
that every module gets past the tables, and is filed as `REQ-20260909T1825Z-7b60`.
