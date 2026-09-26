# Testing strategy

There is no specification for most of what orbistoun implements. The semantics are
undocumented, so "is this correct?" rarely has a cheap answer, and the test strategy is built
around that.

## Sources of ground truth

Four sources, in order of preference. A change that cannot be justified from one of them says
so in its commit message.

### 1. FreeBSD source

The target kernel is FreeBSD-derived, and much of libkernel is POSIX under vendor naming.
FreeBSD source is lawful, citable and the strongest reference available. When implementing a
libkernel function, find the FreeBSD analogue first and name it in a comment. This is why
`orbistoun-kernel` needs less guesswork than any other crate.

### 2. Framebuffer diffing

For the GPU layer: render a frame, compare it numerically against a reference, and get a
number. It is the one cheap, mechanical correctness signal in the codebase, which makes the
GPU crates the best target for tooling and automation.

### 3. The guest itself

A one-bit oracle per call site. Return success: does the guest proceed? Return an error: does
it bail? The answer is bisectable, and `StubPolicy` is runtime data so that a question costs
an edit to a TOML file and a relaunch, not a rebuild.

Each query costs a boot, and it constrains only behaviour the guest observes and checks, so it
converges on "correct for this title" rather than correct. Use a prior (a FreeBSD analogue, a
name, observed argument usage) to choose what to try first rather than bisecting blind.

### 4. Instruction test suites

`SingleStepTests` and `ProcessorTests` give per-instruction JSON with full pre- and post-state
for retro CPUs. They do not cover this target. Their use here is validating tooling: an
automated approach that cannot pass a suite where the answers are known is not trusted where
they are not.

## What gets tested

The high-value targets are the pure ones with concrete contracts, written test-first:

| Crate | What is pinned |
|-------|----------------|
| `orbistoun-nid` | Hash stability, suffix sensitivity, unknown-NID handling |
| `orbistoun-mem` | ABI alignment rules, overlap detection in both directions |
| `orbistoun-hle` | NID resolution, policy override isolation, loud-by-default |
| `orbistoun-elf` | Truncation, bad magic, wrong class, honest failure on vendor data |
| `orbistoun-core` | Error-code round-tripping, placeholder/real code separation |
| `orbistoun-libc` | ISO C and POSIX behaviour at the edges, against a real specification |
| `orbistoun-shader` | Decode against a reference disassembler, and against bytes that are not instructions: random, degenerate, truncated, ragged |
| `orbistoun-translate` | Per-instruction behaviour, executed and compared rather than asserted structurally |
| `orbistoun-gpu` | Submission handling: a shader that will not translate is reported, a window that runs out refuses |
| `orbistoun-names` | A generated index has a specific answer; a confirmed name round-trips through the hash |
| `orbistoun-report` | The progress verdict, and that a differing-conditions comparison is labelled as measuring a settings change |
| `orbistoun-probe` | Every captured transcript parses, and nothing grades above an assumption without an asserted target |

Every test states the property it protects in a comment. A test survives a refactor that
preserves the contract and fails when the contract changes.

Tested lightly on purpose: the CLI's output formatting, and anything whose failure is
cosmetic. Effort goes to the layers where a wrong answer is silent.

## Vacuous loops

A check that iterates over what the code produced and validates each item passes when the
code produces nothing. The loop body never runs, every assertion inside it is vacuously
satisfied, and the report is green. `every_decoded_operand_appears_in_the_reference` in
`orbistoun-shader` has this shape: an instruction that decodes to an empty operand list
produces no comparisons.

The shape to watch for is a test whose assertions all live inside a loop over produced data.
It checks that what was produced is right, and says nothing about whether anything was
produced.

**The converse inventory** is the rule for such a test. Not "at least one item", which drifts
into meaninglessness, but an exact list: the set of cases producing nothing is exactly this
written-down list, each entry with its reason. Closing a gap fails until the entry is deleted,
and opening one fails until it is added and justified. For any "for each X found, check X"
test, ask what it reports when nothing is found; if the answer is "it passes", write the
converse.

## Validation separate from effect

`orbistoun-mem` separates validation from mapping: `validate()` is a pure function over the
ABI rules, and the effectful `reserve()` calls it first. The rules are fully testable without
touching the host address space.

Prefer that shape wherever it fits: a pure decision function plus a thin effectful wrapper.
Where most effects are hard to test, it keeps coverage meaningful.

## Running

```bash
./bin/orbistoun test            # the suite, under nextest when installed
cargo nextest run --workspace   # directly
cargo test --doc --workspace    # doctests, which nextest does not run
```

nextest runs each test in its own process, in parallel. It does not run doctests, so the gate
runs them separately; the `guest_module!` contract is a doctest. `./bin/orbistoun check` runs
both, then re-runs the device-dependent tests with output shown ([BUILDING.md](BUILDING.md)).

## Coverage

`cargo llvm-cov --ignore-run-fail` continues past a failing test binary but records nothing
from it. The profile is written when a process exits normally, and a `libtest` failure exits
by a path that does not write it. Every file covered only by that binary reports `0.00%`, and
every file covered partly by it reports whatever other binaries reached.

A zero in a coverage report therefore means either "nothing tests this" or "the binary that
tests this failed". Fix the suite before reading a coverage number, or skip the failing test
explicitly with `-- --skip <name>`, which keeps the rest of that binary's profile. A baseline
taken while any test fails is not a baseline.
