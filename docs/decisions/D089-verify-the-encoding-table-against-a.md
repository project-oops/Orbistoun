# D089 - Verify the encoding table against a reference disassembler

**decided** · 2026-08-19

`orbistoun-gen fixtures` compiles shaders from source this project wrote, and
`crates/orbistoun-shader/tests/differential.rs` checks our decode against what LLVM's
disassembler says is in them.

D085 shipped a table that was transcribed and unverified, with no way to check it
short of waiting for real shaders from a real title - which is hundreds of functions
away. This closes that, today, with no console and no title involved.

**The assertion is instruction offsets, not counts.** An offset is the sum of every
length before it, so agreement across a hundred instructions means the lengths are
right, and the first offset where the two disagree names the instruction whose
encoding is wrong. Counts can coincidentally match while every boundary is wrong.

**Result: 126 instructions across 10 fixtures, every boundary matching.** All
seventeen families are covered - fourteen by compiled fixtures, and SOPK, MTBUF and
VINTRP by hand-written assembly, since nothing the generator can persuade the compiler
to emit produces those three (D105).

**The provenance line, drawn deliberately.** The disassembler is used to *detect* that
an entry is wrong; correcting one is done from the published AMD document. Differential
testing against another implementation is ordinary engineering; reading its tables to
source the right value is deriving from it. Worth stating because the temptation is
strongest exactly where the answer is hardest to look up.

**Fixtures are committed**, so LLVM is not a test dependency. The binaries were compiled
from source in `tools/shader-fixtures/` - generated, never extracted, the same rule
every other fixture here follows.

