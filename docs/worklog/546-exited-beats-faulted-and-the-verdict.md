# 546. Exited beats faulted, and the verdict that proved it

**2026-09-14** - the tiebreaker D685 named and did not build

## What landed

`ranking_key` now carries how the run ended, directly after the rung:

```rust
(reach, exited_deliberately, imports, answered, standing, frames, calls)
```

Reasoning and the accepted cost are in D686. The short version: it sits *above* `imports`, which that
function otherwise forbids, because the case it exists for is a run whose import count fell **because
it stopped correctly**.

## Verified on the guest, not just in a test

Re-running `obscene-payload`:

| | before the tiebreaker | after |
|---|---|---|
| verdict | `BACK - reaching less of the interface than it did` | **`same - nothing moved`** |
| outcome | `the guest called exit` | `the guest called exit` |

The six calls and one import the payload "lost" were only ever reached by running past its own
refused `exit`. The metric had been counting a program falling off the end of itself as progress, and
now does not.

## Still not fixed, and worth being plain about it

`same` is not `better`, and `beats` requires strictly greater - so the run **still does not overwrite
its record**. `compat/obscene-payload.toml` remains the four-day-old `0x5e2d`. The verdict is honest
now; the record is still stale.

Whether a run should record on "different and not worse" is a question about what a record is *for*,
and it is not answered here. It is the third time this session that a correct change has been
invisible in the file meant to hold it.

## The boundary between D685 and D686

The same objection - *our own guests do not fault and every game does, so ranking the ending high
sorts the probes above the titles* - was refused in D685 and accepted in D686. The difference is worth
keeping straight:

- D685 was about the **rung**, which is trivially reachable. A program whose first instruction is
  `exit(0)` reaches it, and above `Flipped` it would have outranked a title rendering frames.
- D686 is about a **tiebreaker consulted only at equal rung**. A trivial exit is compared at `Exited`
  and loses on imports to everything else there. It cannot overtake anything that flipped.

`a_trivial_exit_does_not_outrank_a_title_that_rendered` is what holds that line, and it still passes.

## Surprises

**A `perl` substitution edited a `use` statement into nonsense.** Replacing a qualified path with a
bare name across a line range also hit `use orbistoun_overrides::DELIBERATE_EXIT;`, turning it into
`use DELIBERATE_EXIT;`. Fourth self-inflicted text-munging wound of the session; the pattern is
always the same - a blind range replace where the range contains a thing of a different kind.

**The const had to move crates to make this possible.** `Status` cannot read the rung to learn how a
run ended, because a run that flipped *and* exited is recorded as `Flipped` - the rung is the
stronger claim and deliberately overwrites the weaker one. So the outcome string is the only place
the fact survives, and the constant identifying it moved from `orbistoun-report` down to
`orbistoun-overrides` where `Status` can reach it. D685's design is what forced D686's plumbing.
