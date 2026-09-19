# 706. The Escape Hatch: the four conditions an autonomous loop halts itself on

**2026-09-19** — inbox `-4a1b`: THE_LOOP's try-measure-verdict cycle is mostly progress a loop can
make alone, but four situations are not, and a loop that keeps grinding on them wastes boots and risks
the "Kyty trap" - a hallucinated stub that compiles and reads like progress. This builds the Escape
Hatch that recognises the four and stops: a new `escape` module in `orbistoun-turn`.

## The policy

`escape::EscapeHatch` observes a loop's attempts on one finding and trips on the first escalation
condition, then stays tripped so the loop halts:

- **`ArchitecturalWall`** - a wall no retry passes, carrying which kind: an unknown GPU packet opcode,
  an untranslatable shader instruction, or an ABI boundary violation.
- **`SpinDeadlock`** - the guest stuck in its own synchronisation, calling the host nothing and
  reaching nowhere (the trace's `quiet` signal, D645).
- **`Regression`** - a change that reached *less* than the baseline: verdict BACK, worse than doing
  nothing, not to be retried into (D129).
- **`RetryExhaustion`** - `RETRY_LIMIT` (3) attempts on one finding with no further between them.

The order is the priority: a wall or a spin is a hard stop the instant it is seen; a regression next;
retry exhaustion last, for the case where nothing broke but nothing moved. Deciding *which* tripped is
a pure function of the attempts observed - the whole of it is testable with no guest.

On a trip the hatch **returns** an `Escalation` (the `[ESCALATION-NEEDED]` record a person reads,
naming the trigger, the finding, the reach and the attempt count) and says the inert trial patches are
rolled back - a patch is a diff proposed, never applied, so rolling one back is discarding it
unproposed. It does **not** write the worklog: [this crate writes to no tracked file](D291) - it hands
the record back and the caller records it, the same split `promote` already makes.

## Made to fail, each direction

Unit tests, all against simulated attempts (the acceptance): a wall trips at once, halts, and rolls
its patch back, and a later `observe` cannot un-halt it; a spin trips as a deadlock; an attempt below
the baseline trips as a regression; three no-further attempts exhaust the retries at exactly the limit;
a **further** attempt resets the retry count so a slow-but-moving loop does not trip; and with no trip
the patches are handed straight back rather than discarded. Each trigger is watched tripping and the
no-trip and reset paths are watched *not* tripping.

## What is deliberately not here

- **The autonomous hardware-probe hook** (the request's wants item 1: query obSCEne over `pros` on the
  live console when a finding lacks known behaviour) is not built and not wired. It touches the PS5,
  and a loop does not touch the console - a person triggers a run. It is not in the acceptance either.
- **The live-loop wiring and the trace→`Attempt` classifier** are the caller's: `Attempt` is
  constructed from a run's trace (reached from distinct imports, spinning from `quiet`, a wall from an
  ABI report or an untranslatable submission), and the module documents that mapping. The policy and
  its tests are the unit this delivers.

## Gate state

`./bin/orbistoun check` passes end-to-end (all checks passed): `orbistoun-turn` tests 93 pass (eight
new in `escape`), clippy `-D warnings` clean, fmt clean, prose exit 0, `status --check` exit 0, doc
gate clean, identity scan clean. Corpus unchanged. No commit.
