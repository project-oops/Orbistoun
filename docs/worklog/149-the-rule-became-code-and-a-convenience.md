# The rule became code, and a convenience script nearly corrupted the fixtures


D225 was a document. It is now `Use`, `usable()` and `Asked::knowledge()`: a live answer
becomes a knowledge entry carrying the arguments it was asked with, the grade, and the
caveat that it was asked under the probe's state rather than the guest's.

**The carve-out keys on the return kind.** `Returns::Status` passes to the guest; everything
else - a handle, a pointer, and *anything unknown* - is recorded and withheld. Unknown is
not permission: not knowing what a function returns is exactly when handing its value over
is most dangerous.

**A death records the death and never a value.** No `edge_cases`, no grade, and an assumption
saying the call did not answer. That is the shape of most first attempts and it is a fact
about the function worth keeping.

`orbistoun ask --as-knowledge` renders it. Only for `call`: a `read` establishes what memory
holds and a `report` establishes a suite's results, and filing either as a return value would
put a byte count where a function's answer belongs.

The GUI is built at `target/release/orbistoun-gui.exe` for testing. It has never been run
by this thread, and that remains the limit of what can be said about it.

### Surprises

**The repository's own check caught me five continuations deep.** `cargo fmt` collapses a
line-continued string literal and bakes the source indentation into the *rendered* text, so
a message reading correctly in the editor prints with twenty spaces mid-sentence. That had
happened repeatedly before the check existed; this time it failed the gate instead, and
twelve literals in this crate were converted to `concat!`.

**The script written to fix them introduced a worse bug than the one it fixed.** It appended
a separator space to every piece - correct for prose, corrupting for wire data, because a
fixture line ending `\n ` puts a stray space at the start of the next record. Seven fixtures
were silently altered. The compiler caught an unrelated problem first, which is the only
reason it was noticed at all: a test fixture that still parses but no longer says what it
said is not a failure anybody sees.

**Four literals were safe and were changed anyway.** `let text = "\` with content at column
zero has no indentation to bake, so `cargo fmt` could not have mangled it - but the check is
a regex over source lines and cannot tell the two apart. Folding them was cheaper than
arguing, and a rule needing case-by-case reasoning is one that eventually gets reasoned
about wrongly.

**`concat!` cannot carry inline format captures.** `{name}` resolves at parse time against a
literal, and a concatenated literal is not one. Three call sites needed positional
arguments - a small thing, and not obvious until the compiler says so.

