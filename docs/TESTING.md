# Testing strategy

The central difficulty of this project is that **there is no specification**. Most
of what needs writing is undocumented semantics, so "is this correct?" usually has
no cheap answer. Everything below is organised around that.

## Where ground truth actually comes from

Four sources, in order of preference. If a change cannot be justified from one of
them, say so in the commit message.

### 1. FreeBSD source

The target kernel is FreeBSD-derived and a large fraction of libkernel is POSIX
with the vendor naming. Lawful, citable, and the strongest reference available. When
implementing a libkernel function, look for the analogue first and name it in a
comment.

This is why `orbistoun-kernel` should need less guesswork than any other crate.

### 2. Framebuffer diffing

For the GPU layer: render a frame, compare numerically against a reference, get a
number. The only cheap and mechanical correctness signal anywhere in the codebase,
which is why the GPU crate is the best target for tooling and automation.

### 3. The guest itself

A one-bit oracle per call site. Return `Ok` - does the guest proceed? Return an
error - does it bail? Bisectable, and the reason `StubPolicy` is runtime data: you
answer the question by editing a TOML and relaunching, not by rebuilding.

Two limits worth being honest about. Each query costs a boot, so it is expensive;
and it only constrains behaviour the guest actually *observes and checks*, so you
converge on "correct for this title" rather than correct. Use a prior - a FreeBSD
analogue, a name, observed argument usage - to choose what to try first rather than
bisecting blind.

### 4. Instruction test suites

Total ground truth, but only for retro targets: `SingleStepTests` and
`ProcessorTests` give per-instruction JSON with full pre/post CPU and memory state.
Irrelevant to this target directly, and genuinely useful for one thing -
**validating tooling before pointing it at something unverifiable.** If an automated
approach cannot pass a suite where the answers are known, it should not be trusted
where they are not.

## What gets tested here

The high-value targets are the pure ones with concrete contracts, and they are
written test-first:

| Crate | What is pinned |
|-------|----------------|
| `orbistoun-nid` | Hash stability, suffix sensitivity, unknown-NID handling |
| `orbistoun-mem` | ABI alignment rules, overlap detection in both directions |
| `orbistoun-hle` | NID resolution, policy override isolation, loud-by-default |
| `orbistoun-elf` | Truncation, bad magic, wrong class, honest failure on vendor data |
| `orbistoun-core` | Error-code round-tripping, placeholder/real code separation |
| `orbistoun-libc` | ISO C and POSIX behaviour at the edges - the one crate here with a real specification |
| `orbistoun-shader` | Decode against a reference disassembler, and against bytes that are not instructions at all - random, degenerate, truncated, ragged |
| `orbistoun-translate` | Per-instruction behaviour, executed and compared rather than asserted structurally |
| `orbistoun-gpu` | Submission handling: a shader that will not translate is reported, a window that runs out refuses |
| `orbistoun-names` | A generated index has a specific answer; a confirmed name round-trips through the hash |
| `orbistoun-report` | The progress verdict, and that a differing-conditions comparison is labelled as measuring a settings change |
| `orbistoun-probe` | Every captured transcript parses, and nothing grades above an assumption without an asserted target |

Every test states the property it protects in a comment. A test should survive a
refactor that preserves the contract and fail when the contract changes.

## The failure mode a passing suite hides

A check that iterates over what the code produced and validates each item **passes when
the code produces nothing**. The loop body never runs, every assertion inside it is
vacuously satisfied, and the report says green.

This is not hypothetical here. `every_decoded_operand_appears_in_the_reference` compares
each decoded operand against a reference disassembly, and it is the test the whole
differential fixture set exists to support. For a period it was green while
`v_mov_b32_e32` - the most common instruction in any shader - decoded to a mnemonic and an
**empty operand list**, because an unsolvable opcode produces no operands and no operands
produce no comparisons.

The shape to watch for is a test whose assertions all live inside a loop over
model-produced data. It is testing that what was produced is right, and saying nothing
about whether anything was produced.

**The fix is a converse, and the useful form of it is an exact inventory.** Not "at least
one operand", which drifts into meaninglessness, but: the set of things producing nothing
is *exactly* this written-down list, each with its reason. Closing a gap then fails until
the entry is deleted, and opening one fails until it is added and justified. Both
directions are load-bearing - a list that only grows is a list nobody prunes.

The same reasoning applies to any "for each X we found, check X" test: ask what it says
when nothing is found, and if the answer is "it passes", write the converse.

## The pattern to copy

`orbistoun-mem` separates **validation** from **mapping**: `validate()` is a pure
function over the ABI rules, and the effectful `reserve()` calls it first. The rules
are therefore fully testable without touching the host address space.

Prefer that shape wherever it fits - a pure decision function plus a thin effectful
wrapper. In a codebase where most effects are hard to test, it is what keeps
coverage meaningful.

## Running

```bash
cargo nextest run --workspace
```

`nextest` over `cargo test`: the conformance suite will be thousands of tiny cases
and nextest runs them process-per-test in parallel. It does **not** run doctests, so
CI runs `cargo test --doc --workspace` separately - the `guest_module!` contract is a
doctest and is worth keeping.

## What is tested lightly, on purpose

The CLI's output formatting, and anything whose failure mode is cosmetic. Effort
belongs on the layers where a wrong answer is silent.

## Measuring coverage: a failing test destroys its binary's numbers

`cargo llvm-cov --ignore-run-fail` lets a run continue past a failing test binary. **It
does not give you partial data from that binary - it gives you none.** The profile is
written as the process exits normally; a `libtest` failure exits through a path that does
not write it, so every file covered *only* by that binary reports as `0.00%` and every file
covered partly by it reports whatever the other binaries happened to reach.

Measured, not assumed. With one unrelated unit test failing in `orbistoun-hle`:

| file | reported | actual |
|------|----------|--------|
| `knowledge.rs` | 35.16% | 97.14% |
| `learned.rs` | 0.00% | 88.89% |
| `lib.rs` | 0.00% | 92.50% |
| crate total | 23.18% | 94.92% |

So a zero in a coverage report is two different facts wearing the same number: "nothing
tests this" and "the thing that tests this did not finish". They need opposite responses,
and the second one silently invites writing tests that already exist.

**Fix the suite before reading a coverage number, or skip the failing test explicitly** -
`-- --skip <name>` keeps the rest of that binary's profile. A baseline taken while anything
fails is not a baseline.
