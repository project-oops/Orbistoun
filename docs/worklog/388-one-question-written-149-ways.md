# 2026-09-04 - (/loop) One question, written a hundred and forty-nine ways

```
719 questions / 275 premises / 40 shared   ->   713 / 135 / 27
suites 126   clippy/fmt/identity clean on both repos
```

Twenty-fifth cron tick, working the named axis: audit the 40 shared premises for the D537 split -
**is this a question for a console, or a statement about orbistoun?**

## The big one was invisible to the grouping

D538 warned that 275 was a **floor**, because grouping is word-for-word and a premise written two
ways counts twice. The delegation question was written **149 ways**: `libScePosix`'s POSIX
spellings each resolve to the vendor-named function beside them, and every entry asked whether
the two behave alike *with the target's name inside the question*. A fifth of the ask list.

Two things in one sentence again. `Resolves to X, and this describes that function` is a fact
about what orbistoun does, and for **122 of the 149 the question was the only place the delegate
was named** - so this had to be a move into `edge_cases`, not a deletion. The remainder, *the two
spellings are assumed to behave alike*, needs no name: the entry is the subject. Split, they are
one premise across 149 functions.

## Fixed at the generator, not at the data

122 of them were generated - `orbistoun-gen knowledge` built the sentence with `format!` and
interpolated the target, so editing the TOML alone would have let the next new delegation put it
straight back. The wording is now `orbistoun_hle::knowledge::DELEGATION_ASSUMPTION` and the
generator pushes that constant.

`the_delegation_question_is_asked_in_one_wording` gates the rest, made to fail by reverting one
entry to the old form.

## The heaviest misfiling, sharpened rather than deleted

24 entries, 101,511 calls: *"The failure return is this project's placeholder rather than an
errno."* The placeholder is a **settled choice** - it avoids the high bit precisely so it can
never be mistaken for a real value. No console can confirm it.

Replaced with the question one step along: **which convention the platform's POSIX-named exports
use on failure** - POSIX's own, or the `0x8002_0000 | errno` encoding D398 measured on their
vendor-named twins. One provoked failure on hardware answers it. The count did not drop, which is
the point: this axis is about the list saying true things, not about shrinking it.

## Six that were facts pretending to be questions

`strtod`/`strtof` - *"which is what the standard specifies"*, in entries that are `published` and
cite ISO C 7.22. And four `scePthreadMutexattr*` entries carrying a width that D272 established
by **watching a whole-word write clobber a loop counter**; `libSceSystemService` already files
the same fact as an edge case.

## What is not done

27 shared premises remain. The next defect down is a different one: cross-references that do not
stand alone - *"Same delivery caveat as posix_sigemptyset."*, *"As `_open`."* - readable in the
file, useless in a queue handed to a console.

Nothing here moves the wall. Third tick of one finding at three levels: D537 (records said
nothing), D538 (the list could not see it repeated itself), D539 (the repetition, at its source).

Decision: [D539](../decisions/D539-one-question-written-a-hundred-and-forty-nine-ways.md).
