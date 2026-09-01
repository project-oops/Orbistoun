# D355 - A turn that measured a contract and wrote nothing


**decided** · 2026-08-27 · asked why three diagnosed titles produced no patches

Three titles were turned in one sitting. One found a real contract -
`sceLibcMspaceMalloc` answering the code the guest followed, zero reaching 25 against 13 -
and **nothing was written**. The finding existed as terminal output, which `CLAUDE.md` names
as already lost.

The cause is a caution applied to the wrong act. Two flags:

| flag | what its own doc says |
|---|---|
| `--record` | *"deciding to change a **tracked** file stays a deliberate act with a diff"* |
| `--apply` | *"**Not a tracked file**... deleting it is a complete undo"* |

`--apply` is gated by the tracked-file caution, and what it writes is not tracked. So a turn
given neither flag - the ordinary case - persisted nothing at all.

**Emitting and applying are different acts and only one needed gating.** A proposal is a file
nothing applies, in an untracked directory, undone by deleting it. Applying changes what the
next run does, which is the act that needs an oracle behind it. `turn` now always writes what
it measured to `patches/`, and `--apply` still gates the policy change (D322).

### What had to be fixed to make it possible

`unimplemented_calls` stripped `library::` from every candidate, because `ORBISTOUN_RETURN`
splits its value on `:` and cannot express a qualified name. True of the variable, and it
threw the library away for everything downstream - so the measurement could not say which
knowledge file it belonged in and the proposal was skipped. Stripped where the axis is built
now, which is the only place that needs it.

### The bug the fix uncovered, which was already there

Adding recording to `turn` filed this:

```
[experiment]  outcome = "ran to the time limit"  imports = 25  standing = 55
```

For a title that reaches **13**. The number was bought by a reserved region the guest never
asked for - a diagnostic run, recorded as a compatibility claim. D227 says it directly: *an
intervention that moves a wall is not a diagnosis*.

`record_compat` now refuses an intervened run. **The hazard was already there for `run`** -
anything with a diagnostic set would have filed the same kind of number - and nothing had
exercised it until a turn did.

### And the gap I diagnosed did not exist

`turn` was said to be missing the recording `run` does. It is not: `GuestTrial` shells out to
`orbistoun-cli run` for **every boot**, so each boot already records itself - the baseline
honestly, and the intervened ones now refused. The `record_compat` added to `cmd_turn` was
redundant, read the *last* trace (a diagnostic), and printed "not recorded" on every turn
while the baseline had recorded fine. Removed.

Worth keeping because the investigation was worth more than the diagnosis: a claimed gap that
was not real led to a real bug two layers away, and the way it surfaced was the guard printing
"not recorded" while the file changed anyway.


