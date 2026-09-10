# 459. The branch that printed nothing

**2026-09-09** - directed, continuing 458

Bus idle an eighth pass. The queued item from 458 built and measured.

## One line, and it explained itself

458 found a 44% import regression in PPSA28061 that no instrument reported. The reason:

```rust
// Nothing to say. The record already holds something as good, which is the ordinary
// outcome of a run that changed nothing.
Ok(Kept::NotBetter { .. }) => {}
```

`NotBetter` covers **two** states. *As good as the record* really is nothing to say. *Worse than
the record* is a regression - and it printed for neither, throwing away the previous status handed
to it in the same pattern (D635).

Now:

```text
below the best ever recorded for this title: 26 imports and 334 calls, against 47 and 933 on 2026-08-23
reached entered where the record reached entered
this run had 20s against the record's 12s, so time is not the explanation
the record is not overwritten - it is a best-ever, and this run is not one
```

The third line is what makes it usable. A shorter run legitimately reaches less, so a bare "fewer
imports" would cry wolf on every quick run and be ignored by the third one. Printing both limits
settles that before anybody goes to look - and here it makes the finding stronger: twenty seconds
against twelve, and still twenty-one imports short.

The rule is split out as `is_below_best` and tested on all four cases, both negatives included: a
run equal to its record must stay **silent**, because a line that fires on equality is one people
learn to skip - which is how the silence it replaces survived. Standing is deliberately not
consulted: implementing a function the guest already called raises standing while moving no import
and no call, and reach is what this line is about.

Watched firing on PPSA28061 and watched staying quiet on PPSA21564, which is at its own best.

## Surprises

- **The comment was accurate and the code was wrong.** It described one of the two states the
  branch covered, correctly, and the reader's eye - mine included, twice - completed the sentence
  for the other.
- **The previous status was already in hand.** `Kept::NotBetter { slot, previous }` carries it, and
  the pattern discarded it with `{ .. }`. Nothing needed threading; the data was there.

## Next

- Bisect PPSA28061 against the build of 2026-08-23. The record carries the date and the instrument
  now says when a run is short, so the method is mechanical.
- The three relocation walls (D633), waiting on scope.
- `ORBISTOUN_DLSYM_STUBS` as a default (D632), waiting on a decision.
- The declined-syscall casualties and `sceAgcCreateShader`, waiting on the two open requests.
