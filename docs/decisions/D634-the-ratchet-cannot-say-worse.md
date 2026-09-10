# D634 - The ratchet cannot say "worse"

**Status:** measured
**Date:** 2026-09-08

## A 44% regression, sixteen days old, that nothing reports

PPSA28061 reaches **26 distinct imports and 334 calls** today, stably across three runs, and stops
because the guest calls `abort`. Its committed record says:

```toml
[status]
reach = "entered"
outcome = "image+0x43c4"
imports = 47
calls = 933
standing = 94
measured_on = "2026-08-23"
```

**Forty-seven imports, on a build from sixteen days ago.** Twenty-one imports and six hundred calls
have gone, and the outcome changed from a fault to a deliberate abort.

Verified not to be this session's doing: `git stash -u`, run, restore - a clean tree at `HEAD`
reaches the same 26 and aborts the same way. It has been like this for some time.

## Why no instrument said so

Two instruments look at this and **neither can express it**:

- **The compat record is a ratchet.** `beats` gates every write, deliberately and for a good
  reason - *"a run under a looser stub policy reaches further by construction, so ranking on the
  numbers alone would let one line of configuration permanently overwrite an honestly measured
  entry"*. So the record keeps the best and a worse run leaves no trace. (Its own test uses
  `status(Reach::Entered, 47, 933)` as the honest record, which is this title's number.)
- **The run verdict compares against the previous run.** Once a regression is the new normal, every
  subsequent run reports `(+0)` and `same`, forever.

A ratchet catches improvements and silently tolerates regressions the moment they stop being new.
Between them, the two instruments answer "did this run improve on the last one" and "what is the
best ever seen", and neither answers **"is this build below its own record"** - which is the
question a regression is the answer to.

## The shape, again

This is the fifth instrument this session that could only say one of the two things it appeared to
say: a differential counting items as findings (D624), a dump that printed a drop as absence
(D623), one that never printed at all (D625), a resolver whose stated invariant held for two-thirds
of its table (D632), and now a progress record that cannot go down.

Every one was found by asking what the instrument *cannot* report, rather than by reading what it
did.

## What to do about it

`report_progress` has the previous run and this one; it does not have the committed record. Giving
it that, and printing one line when a run falls short of the best ever measured for that title,
would have surfaced this the first time it happened.

Not built yet - recorded first, because the finding is worth more than the fix is urgent, and a
rushed instrument is what this record is about. The regression itself needs bisecting against the
build of 2026-08-23, which is a separate job with a clear method: the record carries the date.
