# 741. A copy from a null source is now a named fault finding, not just a trace operand

**2026-09-21** — worklog 740 made the copy operands (`src`/`n`) visible in the trace line; this tick
promotes the pattern one level up, from a value a reader can spot to a **named finding the report
states itself**. When a byte copy faults reading its source, the fault report now says so — names the
copy, computes where in its source the read landed, marks a null-page source explicitly, and routes
the action to whatever produced the pointer rather than to the copy. This is the detection that would
have short-circuited PPSA02664's eleven-worklog misreading on the first run, and it is built to catch
the whole class, not this one title.

## Why this, and why here

Two constraints shaped the choice of work. The standing instruction is to make this class of bug
**findable without oracle guidance** — the report should carry the diagnosis, not a person matching
values by eye. And a concurrent session is actively editing the AGC code path this title's wall runs
through (`agc.rs`, `agc_driver.rs`, `packet.rs`, the knowledge TOMLs). Making a *code* change there
this tick would collide with a live edit; making the **report** better does not, and it is squarely
the tooling the instruction asks for. So the tick stays in `orbistoun-report`, where 740 already
added the operands.

## What the report now does

`faulted()` gains a sound reading of the commonest graphics wall — a copy from a null (or wild)
source:

- **`copy_reading_fault`** finds the most recent `memcpy`/`memmove` on the faulting thread whose
  recorded source range `[src, src+n)` covers the fault address. It reads `arg1`/`arg2` **captured at
  the call**, never the mid-copy register dump where `rdx` is a *remaining* count — the exact misread
  that cost this wall eleven worklogs (740). Most-recent-first, so the still-running copy (recorded
  with no return) is the one it names.
- **`copy_source_lead`** turns that into the lead evidence line: `>> libc::memcpy faulted reading its
  source: given src 0xa8 n 0x50, whose base is in the null page (0x0 + 0xa8); the faulting address is
  byte 0x0 into that source`. Values and arithmetic only (principle 3) — where the read landed, and
  whether the base is null-page — never a cause.
- **The action exonerates the primitive.** When a copy is the fault, the action reads: *"`libc::memcpy`
  is a faithful byte copy — it moved what it was given. The gap is whatever produced its source
  pointer `0xa8`; read the calls on this thread before it for the lookup or allocation that should have
  answered a valid pointer there, not the copy itself."* A copy line invites the confident-wrong turn
  of "fix memcpy"; the report now refuses it and points upstream, where the null base actually comes
  from (a wall is orbistoun's until proven otherwise).

## Held to the negative

Per principle 3 (a guard is not finished until it has been made to fail), the test pins both
directions. A copy whose recorded source range **covers** the fault is named as reading it, with the
null-page base and the byte offset; a copy whose source is a valid stack pointer nowhere near the
fault is **not** named — so it is a range match on the recorded argument, not "any memcpy in a
faulting tail". The finder reads `arg1`, so it cannot be fooled by the register dump the way a human
reader was.

## Scope and provenance

No AGC/driver code touched — the change is confined to `crates/orbistoun-report/src/diagnose.rs`
(`is_source_copy`, `copy_reading_fault`, `copy_source_lead`, and the `faulted()` integration, with a
test). The line-count lint pushed the evidence arithmetic into `copy_source_lead` beside the finder,
which reads better anyway. This is orbistoun's own diagnostic getting sharper; nothing was taken from
any reference.

## Where this points next

The report now names *which copy* read the null and tells the reader to find *what produced its
source*. The open question is unchanged and now has a tool aimed at it: which creation or lookup
returned the null-based object whose `+0xa8` field is group nine's copy source, where groups zero
through eight source from the stack and succeed. That is an orbistoun-side trace question best
answered once the concurrent AGC edits settle, so a run observes a stable path rather than a
half-applied one.

## Gate state

Changed only `crates/orbistoun-report/src/diagnose.rs`. `cargo test -p orbistoun-report` is 102/102
(the new copy-fault test included); `cargo fmt --check` and `cargo clippy --all-targets` clean on the
crate. `./bin/orbistoun check` is still red on the same three generated-doc drifts as 740
(`PROJECT_STATUS.md`, `PPSA02664-app0.md`/`COMPATIBILITY.md`, `compat-frontier.txt`), all rendering
from `compat/PPSA02664-app0.toml`, a concurrent session's live edit that is not this change and is
left for that session to regenerate. Identity scan clean. No commit.
