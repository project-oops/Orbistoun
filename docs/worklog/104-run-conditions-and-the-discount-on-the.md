# 2026-08-21 - Run conditions and the discount on the headline (D181)


A verdict compared two traces without knowing whether they were comparable. Two settings
break the comparison and both are one line of TOML: the wall-clock `--limit`, which means a
faster machine reaches further on the same build, and `default_return = "ok"`, which makes
every unimplemented function claim success.

The second is a reward hack one line wide. Every number improves at once - the guest stops
checking, runs on, reaches imports it never reached - and nothing has been implemented.
Anything steering by a call count finds it within a few iterations, not from malice but
because it is the cheapest change with the biggest effect.

**Recorded rather than forbidden.** Answering `ok` everywhere is a legitimate bisection
technique and principle 5 exists to keep that loop cheap; what makes it a hack is doing it
unlabelled. `Conditions` rides on the trace and `compare()` reports what changed underneath
a verdict instead of refusing to render one - the numbers are real, they just are not
evidence about the emulator. Build is recorded and deliberately not compared: it would fire
on every release and drown the two that matter.

Every run also prints what it stands on now - 787 of 933 calls answered by an
implementation, 15% on stubs. A call count is progress only to the extent something real
answered.

Demonstrated rather than argued. Same title, same build, one line changed: `same / nothing
moved` became `FURTHER / reached imports it could not reach before`, with the caveat printed
directly under the verdict. The hack still works; it can no longer be read without being
told.

