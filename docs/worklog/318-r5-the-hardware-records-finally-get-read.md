# 2026-09-02 - (/loop) R5: the hardware records finally get read, and the two runs disagree

> **Corrected 2026-09-02 (D480):** the sign-extension divergence recorded below is
> **withdrawn**. The leading `ffffffff` is obSCEne widening a C `int` through `int64_t`
> - `int second = scePthreadMutexTrylock(...)` then `(uint64_t)(int64_t)second` - not the
> console setting the top half of `rax`. A prototype returning `int` reads `eax` and never
> saw the other thirty-two bits, so those records could not have answered the question at
> all. Read at the width they were taken, every value is one orbistoun already produces.
> The width question D398 opened is still open and needs a register-level capture.

```
knowledge entries carrying a hardware observation    0  ->  77
functions created                                    0       (attaching, not inventing)
of those 77, findings the two runs did not agree on       12
```

`orbistoun-gen hardware` joins `try` and `res` on their check id and records what a conformance
run saw about each function it exercised. 2.3 MB of records had been on disk since 2026-08-30
with nothing in the tree reading them.

## The number in the brief was wrong, and the shape was wronger

The plan said "294 valued `res` records". The captures hold **234** in `ps5-full.txt` and 250
in `ps5-imports.txt`. More important than the count:

- **178 of the 234 name `(census)`**, a pseudo-symbol a sweep check uses because it has no
  single subject. Attaching those would file a whole-surface count against whichever name sat
  in the field.
- What is left joins to **52 distinct functions**, every one of which already had a knowledge
  entry. Nothing needed inventing, and nothing was.

## The thing that decided the design

A `res` value's meaning lives in the check, which is C source, not in the record.
`sceKernelWrite` answering `0xffffffff80020009` to a bad descriptor is a fact about the
function. `sceKernelGetProcessTime` answering `0xc3` is the time it happened to be. **Both
arrive in the same field**, and nothing in the record distinguishes them.

I did not have to argue this. **The two captures settle it**: twelve check-and-function pairs
answered *differently* in the two runs -

```text
010-kernel/process-time          0xc3        vs  0x83
010-kernel/process-time-counter  0x469883c2  vs  0x4586cd13
020-memory/map                   0x200374000 vs  0x200b64000
110-modules/list                 0x1f        vs  0x20
```

- timestamps, mapped addresses, allocation sizes, a thread handle, a module count. A tool that
read every value as "what this function returns" would have recorded that
`sceKernelGetProcessTime` returns `0xc3`, and the other capture on the same disk disproves it.
It would have looked like 234 new facts.

So an observation is recorded **quoted, not interpreted**: the check that made it, the verdict,
the value as written, the note, and where to read the line back. The condition stays where it
is written down - in the check - and the entry says so, so anybody can recover it.

## The disagreements are kept, not averaged away

The first cut folded matching outcomes and would have dropped one side of each disagreement.
That is backwards: **a value that moves between two runs of one check is not a constant of the
platform**, which is a real finding and is invisible in either run alone. Those twelve now read:

> obSCEne `020-memory/allocate` did not report the same thing twice, so the value is not a
> constant of the platform: reported pass, value `0x10000` in `ps5-full.txt`; reported pass,
> value `0x7fc000` in `ps5-imports.txt`.

And the 47 that *did* agree across both runs say so by naming both, which is the reproducibility
signal the same mechanism gives for free.

## The field this refuses to set

**`known_by` is left exactly as it was**, and the plan's success criterion - "the `measured`
count reflects the hardware runs already performed" - is deliberately not met.

The field means "how the behaviour recorded above was established". One hardware observation
about one edge does not establish a function's whole recorded contract, most of which came from
a published specification. Promoting an entry to `measured` on the strength of a single check
would make the tier counts read as though the platform had confirmed far more than it has - and
the tier counts are precisely what this project uses to know how much of itself is guessed.
Inflating them is the same over-claim in a report that principle 3 forbids in a stub.

**The tier moves when a record carries its own condition as data.** That is R4, and doing R5
turned it from a good idea into the specific thing blocking the tier. Until then the observation
is itemised and the tier holds.

## What landed

- `crates/orbistoun-gen/src/hardware.rs`, and an `orbistoun-gen hardware` mode reusing R0's
  loader and writer. Every record goes through `KnowledgeFile::merge`, so the format's own
  provenance rules decide admissibility and **one fault stops the whole write** (D180).
- Nine tests, including the two that matter: an observation must not contain the word
  "returns", and a value differing between runs must be kept as a disagreement rather than
  merged to one.
- Idempotent, checked against the real captures rather than only the fixture: running it twice
  leaves 77 edge cases and 658 function blocks, unchanged.

## State

`cargo test --workspace` green - 115 suites, **1930 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean.

Nothing committed. It is 18:07 UK, past the window, and the day holds worklogs 292-318 and
D466-D477.

## A divergence found on the way out, which is R6's case in one line

The `measure` records are shaped far better than the `res` ones - `check | subject | condition
| observation | kind` - and **`kind` is the discriminator `res` lacked**: `code` and `hz` are
constants, `ticks` and `offset` are not. There are 34 in `ps5-full.txt` and **73** in
`ps5-imports.txt`.

Testing R6's premise against exactly one of them:

```text
OBS|measure|015-sync/mutex-unlock-unheld|scePthreadMutexUnlock|unheld-unlock|0xffffffff80020001|code
```

Hardware answers `0xffffffff80020001` - **sign-extended to sixty-four bits**. Orbistoun answers
`u64::from(GuestError::vendor(NOT_OWNER).as_raw())`, which zero-extends: `0x0000000080020001`.

D398 left this open in as many words - *"some calls hand the code back sign-extended and some
do not, in the same run. That is the return width of the individual function, not a property of
the code, so it is not encoded here and belongs with whichever shim returns it."* The measure
records say **which** functions sign-extend - `scePthreadMutexUnlock`, `scePthreadMutexTrylock`,
`sceKernelLoadStartModule`, `sceKernelDlsym`, `sceKernelDirectMemoryQuery` - and orbistoun does
it for none of them. A guest testing the full word, or reading it as a signed 64-bit int, sees a
different answer than the console gives.

Not fixed here, deliberately: the width belongs per function, the two captures need reconciling
against the `res` values that disagree with them, and it wants a decision rather than a sweep.
**It is R6's first red test**, which is the point - a divergence nobody could see becomes a task
with a completion condition.

**Next**: R6 - generalise `ctype.rs` into measure-record-to-generated-test, and add
`also hardware` to `check()`. The supply is larger than the plan assumed: 34 + 73 measure
records across the two captures, each already carrying subject, condition, observation and kind.
