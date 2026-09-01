# A provenance rule that was stated too narrowly, and a refusal defended for the wrong reason


Another thread reviewing this work concluded the instruction set material came from
PS4-era emulator decode tables, and recommended LLVM's TableGen files instead. Both halves
were wrong, in opposite directions.

The material came from AMD document 70648, the RDNA 2 ISA reference, cited in
REFERENCES.md with a retrieval date since it was fetched. The decision log separately
records that shadPS4, GPCS4, KytyPS5 and obliteration were deliberately not opened. The
target is `gfx1030`; the files that thread listed are `gfx900`, a generation earlier, and
none of those paths are in this repository.

So the sourcing was fine and documented. Two things were not.

**The boundary section named two categories where there are three.** "Published
specification" versus "another implementation's source" leaves no room for the case that
actually matters most here: the *vendor's* own open-source contributions. AMD publishes
its ISA guides and contributes the AMDGPU backend to LLVM under a permissive licence -
that is a silicon interface documented so that people will program against it. Refusing it
because emulator source is off-limits is a category error, and it is a cost with no
benefit. Now stated as three categories, with the reason each is in its own row.

**And I defended the LLVM restriction on the wrong grounds.** My first answer ran the
licensing objection and the engineering one together. They are not the same, and the
licensing one is wrong: nothing forbids reading `.td`.

The real reason is oracle independence. The AMD document supplies values; LLVM detects
errors through its behaviour as a black box. Generating our table from LLVM's tables
collapses two sources into one, and the differential test can only confirm that LLVM
agrees with itself. The LDS opcode field is the standing proof - `[25:18]`, where the
document's field table says `[24:17]`, caught because the generator disagreed with the
document and the document's own opcode table settled it.

Stated once, in REFERENCES.md and D206: **LLVM may check a table and may cross-check a
fact; it may not be the thing the table is generated from.**

### Surprises

**Defending a correct decision with the wrong argument is its own failure mode.** The
conclusion held, so nothing looked broken - but an engineering constraint dressed as a
licensing rule loses the argument the moment someone checks the licence, and takes the
correct conclusion with it. Worth more than the documentation fix.

**Reading `.td` as a *third* source is welcome and I had argued myself out of it.** The
hidden condition-code side effects and the division thresholds are `BLOCKED` in `model.rs`
for want of exactly that kind of machine-readable fact. Additive rather than circular, and
now recorded as such instead of being lumped in with the refusal.


