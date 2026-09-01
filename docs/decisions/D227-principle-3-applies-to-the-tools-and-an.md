# D227 - Principle 3 applies to the tools, and an intervention says so on the line


**decided** · 2026-08-25 · asked directly: *"how do we prevent similar mistakes in the
future"*

Five things in one session reported more than their measurement supported:

| Said | Had measured |
|---|---|
| "every entry accounts for what it claims" | only `known_by`, never `found_by` (D213) |
| "every name is accounted for" | true, then skipped the ceiling entirely (D213) |
| "not a pointer" | "not in my list", and the list was a page out (D217) |
| "reached imports it could not reach before" | the instruction pointer moved (D224) |
| "the address was right" | a fault moved under a mapping (D226) |

Plus one outside the tree: a verification that summed `N passed` and never looked at
whether anything failed, reporting 214 green while a test was red.

### They are two failures, not one

**Four are a claim not derived from its measurement** - a hand-written sentence beside a
computation, free to drift from it. None is obvious when written.

**One is different**: reading a change as a confirmation. A mapping moved a wall, the
movement was read as confirming the hypothesis that motivated the mapping, and watching
what the guest *wrote* one run later said the opposite. Post-hoc reasoning with a tool
attached, and it is the one that was mine rather than inherited.

### Principle 3 already covers all of it, one level up

*Honest failure over plausible output* is written about the emulator - stubs returning
success, error codes mistakable for real ones. **Every failure above is the tooling doing
what the principle forbids the emulator to do.** So it now says so, with three rules under
it: a guard is not finished until made to fail; a message naming a cause comes from the
branch that determined it; an intervention that moves a wall is not a diagnosis.

### The mechanism, rather than the resolution

The third rule is the one that needed machinery, because it fires exactly when somebody is
pleased with a result.

`orbistoun-env` now records an [`Effect`] per diagnostic. `DUMP` and `WATCH` **observe** -
the guest runs the program it would have run, so a verdict under them measures the
emulator. `POKE`, `MAP`, `WRITE`, the three fills and `MARK_QUERY` **intervene** - they
change the program being measured.

A run under an intervening diagnostic that reports progress now prints, in the verdict
block:

```
verdict  FURTHER  executed code it could not reach before
         ! this run altered the program, so getting further may mean the
           guest accepted a wrong answer. Check what it *wrote*, not only
           that it moved - ORBISTOUN_WATCH is what answers that
```

Which is the exact line that was missing at the moment the wrong conclusion was drawn.

Three details are deliberate. It fires **only on a progress verdict** - a caveat on every
instrumented run is noise people learn to scroll past, which is how a warning stops
working. The flag is **derived from the registry** rather than listed again, so a
diagnostic added with the wrong effect is wrong in one place. And a **setting may never
intervene**, held by a test: if one ever needs to, that is an argument to have rather than
a field to flip, because every ordinary run would then carry the caveat.
