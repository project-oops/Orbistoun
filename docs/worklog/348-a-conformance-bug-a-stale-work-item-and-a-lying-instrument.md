# 2026-09-03 - (/loop) a conformance bug, a stale work item, and a lying instrument

```
tests            1992  ->  1992
differential      130  ->    146 cases
```

Arrived by hand again - the scheduled wakeup has now failed five times running.

## Sixteen differential cases, and another real bug

`strtol` bases 0/2/16/36, sign-only input, a leading tab, and four `snprintf` truncation
cases around the exact boundary. One failed immediately:

```text
strtol/hex-prefix-no-digits: end_offset is 0x0, expected 0x1
```

`strtol("0x", &end, 16)`. ISO C 7.22.1.4 takes the *longest initial subsequence of the expected
form*, which for `"0x"` is `"0"` - so it **converts**, answers zero, and leaves `end` on the `x`.
orbistoun consumed the prefix, found no digit, and reported no conversion at all. The value was
right by accident; the `endptr` was wrong, which is the one thing `endptr` exists to say. A
caller walking a string would have looped forever. Fixed, D498.

## A work item that had stopped existing

The plan's next item was `/dev/random` and `/dev/urandom`, on the strength of the run asking for
them. **The current run never asks.** That request belonged to the pre-D489 path of 10,884
placeholder calls; the run now dies at ~2,080.

Third time this session a work item turned out to be a fact about a superseded run - after
D494's 222 resolver calls and D488's "+18 from linking". The list needed **re-deriving, not
working through**, and re-derived it is three items long.

## And an instrument that answered 0 to everything

Chasing obSCEne's `decisions` gate, I read its exit code through
`wsl.exe -- bash -lc '...; echo "exit=$?"'` and got 0. Wrote it up as *a gate that passes by
finding nothing* - the exact failure this project keeps cataloguing.

Then ran the control:

```text
false; echo $?     ->  0
(exit 3); echo $?  ->  0
```

**The channel was mangling the command**, not the tool reporting success. Through a script file
instead: `false` -> 1, a bogus subcommand -> 2, the gate -> **1**. It fails correctly, and the
finding was mine.

Rule 3 says validate a tool before trusting its negative. This is the sharper form: **the
control belongs in the same run as the measurement.** `false` costs nothing and would have
caught it before the conclusion, not after.

What survives is smaller and real: obSCEne's `decisions` gate targeted the single-file log it no
longer has, so `verify.sh` had been failing on it. Fixed there (its D314), and on its first
honest run it found **eight index rows with no title at all**.

Then the same lesson twice more in one tick. A `grep -c "error|warning"` over a build log
returned 2, both of them the `-Werror` in the command line. And a shell whose working directory
had drifted to orbistoun answered questions about obSCEne's decision log with orbistoun's -
which read as another session having overwritten fifteen files, right up until `pwd`. **Four
readings in one tick that were facts about the instrument.** Absolute paths from here.

## What is blocked, written where it can be closed

Per the standing instruction that anything without data gets called out:
`obscene/docs/backlog/022-measurements-orbistoun-is-blocked-on.md`, indexed in its BACKLOG.
The two pthread setters, the LoadStartModule-on-a-placed-module question, D398's return width,
and the out-parameter sweep - each with what would settle it, on the side that owns a console.

## State

`cargo test --workspace` green - **117 suites, 1992 tests**, 0 failures. Differential **146
cases, all agreeing**. clippy `--tests` clean, fmt clean, identity scan clean on both repos.

Nothing committed. The day holds worklogs 292-348 and D466-D498.

**Next**: the ±3 call oscillation, and the 95 outstanding hardware measurements -
`035-libc/getpctype` pointers and `106-encoder/symbols` are the big groups.
