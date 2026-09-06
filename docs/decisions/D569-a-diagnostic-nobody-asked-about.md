# D569 - A diagnostic nobody asked about, and the record it overwrote

**Status:** measured
**Date:** 2026-09-04

## What happened

`ORBISTOUN_TAG_PLACEHOLDERS` was declared, and within the hour four titles were run under it. Three
of them **overwrote their honest compatibility records** with numbers measured under a diagnostic.

The guard against exactly this already existed and is emphatic (D227, D355):

```rust
if trace.conditions.intervened {
    println!("  not recorded: this run was under a diagnostic, so what it reached is");
    println!("  a fact about the intervention rather than about the title");
    return;
}
```

It never fired, because `Experiments::intervenes` did not know the new diagnostic existed.

## The shape of the hole

`intervenes` enumerated every field of `Experiments` against its variable's effect, under a comment
that said the right thing about the wrong half:

> Derived from the registry rather than listed again here, so a diagnostic added with the wrong
> effect is wrong in one place instead of two

The **effects** were derived. The **presence list** was hand-maintained, one line per diagnostic,
and a new one was simply absent from it. So the variable was declared, registered, listed by
`orbistoun-cli env`, honoured by the service - and invisible to the one check that decides whether
a run counts.

That is the D220 shape this project keeps meeting: a setting consulted at one site and not at the
site that matters. It has now happened to `ORBISTOUN_COMMIT` (D166), to the stub policy for
undeclared imports (D187), and here.

## The fix, which closes the class

```rust
fn any_intervenes(active: &[(&'static Var, String)]) -> bool {
    active.iter().any(|(var, _)| var.effect.needs_caveat())
}
```

`orbistoun_env::active()` already walks the **whole** registry, and a separate test already
refuses a variable that is declared without being registered. So asking the registry makes a new
diagnostic covered the day it is declared, with nothing to remember.

It errs the safe way, too: a variable that is set but unparseable still counts as intervening, so
the run is refused rather than filed.

## Split, because the test would otherwise have been the flaky kind

The first version of the guard set each registry variable in the process environment and checked
the result. It failed on a variable it had not set - parallel tests share one environment, and
mutating it from a test is flaky by construction.

So the decision is a pure function over a list and `intervenes` is a one-line wrapper that reads
the environment - principle 8's own prescription, *a pure decision function plus a thin effectful
wrapper*. **The wrapper is deliberately untested**: replacing its `active()` call with an empty
slice does not fail anything, a break that was tried and did not fire. That is one call left
uncovered on purpose rather than by oversight, and it is written into the test.

## Putting the records back

Three titles were re-measured honestly. Two of them - PPSA02664 and PPSA03416 - could not be
recorded normally, because the polluted numbers were marginally *higher* and `beats` refuses
anything that is not an improvement. They were replaced with `--force`, which is the case that
option exists for: the standing record was known to be wrong, not merely beaten.

**That is the second time today the "a record only moves up" rule has been the obstacle** rather
than the protection - worklog 412 found PPSA28061 keeping a claim the current build cannot
reproduce. Still recorded rather than fixed: a best-ever record and a current-state record are
different things, and choosing is not a bug fix.

## What this does not establish

**That no other record is polluted.** Three were caught because they were made in the last hour and
could be re-run. Any earlier run under a diagnostic that predates this fix had the same hole, and
nothing here audits the back catalogue.

**Nor that every effect in the registry is right.** A diagnostic declared `Observes` that in fact
changes the program is invisible to all of this - the registry is the single source, so a wrong
entry is wrong once and completely.
