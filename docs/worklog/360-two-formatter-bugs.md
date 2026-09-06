# 2026-09-03 - (/loop) Two formatter bugs, in the padding nobody had tested

```
differential   263  ->  286 cases
libc bugs found this week     +2
```

Arrived by hand again - the seventeenth wakeup that did not fire.

## `sprintf` had no cases, and twenty-three of them found two bugs

```text
sprintf/width-zero-negative: buffer is 30302d3432…, expected 2d30303432…
sprintf/precision:           returned 0x2, glibc returned 0x3
```

**`%05d` of -42 rendered `00-42`.** Padding was applied uniformly after rendering, so a negative
number got its zero fill in front of its sign. ISO C 7.21.6.1 puts the `0` fill after the sign:
`-0042`. Nothing could have caught it by reading - the length is right, the digits are right,
and only the order of two characters is wrong.

**`%.3d` of 42 rendered `42`.** Precision was read, applied to `%s` where it truncates, and to
nothing else. For `d i o u x X` it is the *minimum number of digits*, so `042` and a return of
3. Two more rules came with it: a specified precision makes the `0` flag ignored, and a
precision of zero with a value of zero renders nothing at all.

D511. Both watched failing **separately**, by reverting each half of the fix on its own - one
break would have left the other half unproven.

## Why these cases found what the existing ones could not

`snprintf` has had cases since the differential was built and **every one formats `%d` with no
flags**. The interesting failures in a formatter are not in any one conversion, they are in the
specifier parser. So the format string is a case *input* here, and the twenty-three cases sweep
conversions and flags rather than values - `%d %i %u %x %X %o %p %c %%`, width, left-justified,
zero-padded, precision, and a width narrower than the value.

That shape is worth more than either bug.

## Housekeeping the cases forced

`run` in the differential test grew an `if` block per shape and hit the 100-line ceiling
**twice** while these were being added. Consolidated into one `match` naming which replay each
function takes: adding a shape is now a line, and a reader asking how `memcpy` is replayed has a
list rather than a chain.

Check 11 held again - `run_format` takes its implementation by name from a constant rather than
from the case, but only because `sprintf` is the sole function on that path; the comment says so
in case a second one arrives.

## State

`cargo test --workspace` green - 119 suites, **1998 tests**, 0 failures. **Differential 286
cases**, all agreeing. clippy `--tests` clean, fmt clean, identity scan clean on both.

Nothing committed. The day holds worklogs 292-360 and D466-D511.

**Next**: `vsnprintf` is the last uncovered differential function and needs a `va_list`. The
wide-character family and an interleaved-sequence shape both still need a record-format
decision.
