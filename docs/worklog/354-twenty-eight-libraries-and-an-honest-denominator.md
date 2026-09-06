# 2026-09-03 - (/loop) Twenty-eight libraries, and a coverage figure that got worse on purpose

```
declared / implemented   674/647 (96%)  ->  947/647 (68%)
unresolved imports                 342  ->  117
suites / tests                 117/1993 ->  119/1994
```

Asked for "the remaining eleven libraries". It was **twenty-two from this title alone** - the
executable imports from thirty-five and orbistoun declared fifteen - plus six more that only
other corpus modules import. Twenty-eight declared, 212 names, none implemented.

## The interesting number went down

The 96% was measuring the wrong set: the declared surface was very nearly *what had already
been implemented*, so it could not help looking finished. **947 - 273 = 674, the old declared
total exactly** - nothing was removed, and everything added is a name a guest asks for that
nothing answers.

`status` now says both halves rather than leaving it to be rediscovered:

```text
| Functions declared / implemented          | 947 / 647 |
| Declared in a library that serves nothing | 273 across 30 libraries |
```

Counted by library, not by symbol: one implementation means a library is being worked on, none
means its names have only been written down. D505.

## Homes, so an implementation would not have to move

Audio took `libSceAjm`/`AudioOut2`/`Audio3d`/`AudioIn`; video took `libSceAvPlayer`/`VencCore`/
`VideoRecording`; input took `libSceKeyboard`/`Mouse`/`Ime`/`ImeDialog`; systemservice took the
nine dialog, save-data, content and JSON libraries; gpu took `libSceAmpr`.

**One new crate**, `orbistoun-net`, for the seven networking libraries - sixty-eight functions
with no home, and putting them in one that fit badly would have to be undone. Its header says
what networking is *not* going to be, because a networking stack is the kind of thing that grows
by accident: `docs/SCOPE.md` puts online services out of scope, and what these buy is a title
being visible in a report rather than dying on an unresolved import.

`libSceAmpr` is placed by name association with the graphics submission path and says so.

## And a near-miss worth more than the declarations

The first four runs after came back **2080 calls / 46 distinct, four times** - where the run has
been bimodal all day. That is what a claim of "declaring the libraries fixed the
non-determinism" would have rested on.

Four is not enough to separate that from a coin weighted 60/40. Eight more samples: three at
2077/44, five at 2080/46. **The oscillation is unchanged and D499 stands.** The three-sample
floor earned its place again; four in a row at p=0.13 is not a finding.

## A load-dependent test failure I could not reproduce

`a_timed_semaphore_take_gives_up_and_can_be_rescued` failed once, in a whole-workspace run, on
`assert!(started.elapsed() >= BRIEF)` - a timed wait reporting it gave up before its deadline.
Passed 3/3 alone, then **58 more attempts across 5ms and 80ms deadlines did not reproduce it**.

The leading guess was rounding: `Condvar::wait_timeout` is handed the span remaining to a fixed
instant, and a host that rounds it down would report `timed_out` a fraction early. Unsupported,
so it is written as a guess and nothing was changed to chase it.

What was added is an instrument: `a_timed_wait_never_gives_up_before_its_deadline` exercises the
property **fifty times a run instead of once**, so the next occurrence arrives with a turn
number attached. Weakening the original assertion to get green was the other option and would
have thrown away the only evidence.

## State

`cargo test --workspace` green - **119 suites, 1994 tests**, 0 failures. clippy `--tests` clean,
fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-354 and D466-D505.

**Next**: whether declaring `libSceVencCore` and `libSceVideoRecording` lets the 33 encoder
`unresolved = 0x0` measurements be claimed - the measurement is "the symbol resolves on the
console", and orbistoun now resolves them too, so the claim may be legitimate. Check the
proposition carefully before writing the test.
