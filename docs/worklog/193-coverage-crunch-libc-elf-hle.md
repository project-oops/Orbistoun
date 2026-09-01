# Coverage crunch: libc, elf, hle


159 tests added across six new integration files, all under `tests/` so nothing collided
with work in flight elsewhere in the tree.

| file | before | after |
|------|--------|-------|
| `orbistoun-libc/src/cstring.rs` | 0.96% | 99.04% |
| `orbistoun-libc/src/lib.rs` | 57.95% | 90.50% |
| `orbistoun-libc/src/math.rs` | 0.00% | 73.13% |
| `orbistoun-elf/src/lib.rs` | 49.19% | 73.56% |
| `orbistoun-libc` overall | 59.17% | 90.06% |

Reaching the private functions through the public `implementations()` table turned out to
be the right shape rather than a workaround: a test there exercises exactly what a resolved
import exercises, so a function dropped from the table fails these even while it compiles -
the same bug as a module registered without its implementations (D281).

### One real bug

`strtod` and `strtof` advanced the end pointer past skipped whitespace **even when nothing
converted**. C says the pointer stays at the original string when no conversion is
performed, and `cstring.rs`'s integer parser already did exactly that - so the two families
disagreed with each other. Two lines, in `math.rs`.

### Two surprises worth keeping

**A failing test binary writes no coverage profile at all.** Written up properly in
`docs/TESTING.md`; the short version is that `orbistoun-hle` was reported at 23% while
actually at 95%, because one unrelated unit test was failing. Every zero in a coverage
report is two different facts wearing one number.

**Two of the tests were wrong where the code was right**, and both were worth the detour: a
dynamic segment outside every `PT_LOAD` is still located through *its own* header, and
`strtok` writes one terminator per token rather than replacing every delimiter. The tests
now assert the real contract and say why.

### One suspicion measured and dropped

`vendor_name_table` and `needed_libraries` resolve `info.strtab` with `vaddr_to_offset`
while `raw_imports` uses `table_offset`, which reads like D247 surviving at two sites. It is
not: attribution on a real commercial module produces `libSceAgc`, `libSceHttp2`,
`libSceAvPlayer` - names orbistoun does not declare, so they came from the module's own
table. Checked before reporting, which is the lesson from the pad-name episode.

### Deliberately not tested

`abort` and `exit` reach `orbistoun_core::stop`, which is diverging and calls
`std::process::exit` - calling either would take the test binary down and report every other
test in the file as never having run. Named in the file's header so the absence reads as a
decision.

