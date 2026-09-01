# D181 - A run records what it was subject to, so a verdict can be evidence


**decided** · 2026-08-21

The loop rests on one inference: run, change one thing, run again, attribute the difference
to the change. That is valid only if everything *else* was identical, and nothing recorded
whether it was. `compare()` rendered `FURTHER` or `BACK` from two traces that might have
been produced under completely different settings, and could not tell.

Two settings break it, both one line of TOML away.

### The wall-clock limit measures the host

`--limit` is in seconds, so the same build on the same title reaches further on a faster
machine. Locally that is a slow leak; the moment results are shared - a contributed
compatibility entry, a second contributor reproducing a finding - two people comparing runs
are comparing their hardware and nothing warns them.

### The stub policy is a reward hack one line wide

`default_return = "ok"` makes every unimplemented function claim success. The guest stops
checking, runs on, reaches imports it never reached before, and dies much later. **Every
number improves and nothing has been implemented** - and worse, the guest is now running on
placeholder answers, so where it eventually dies means nothing.

It is the highest-scoring single change available to anything steering by a call count, so
an unattended loop finds it within a few iterations. Not from malice: it has no concept of
cheating, only of cheapest-change-with-biggest-effect.

**Recorded rather than forbidden.** Answering `ok` everywhere is a legitimate bisection
technique and principle 5 exists to keep exactly that loop cheap. What makes it a hack is
doing it *unlabelled*, so the label is the fix.

### Two numbers, never one

`Conditions` rides on the trace - limit, default stub answer, override count, build - and
`compare()` reports what changed underneath a verdict rather than refusing to render one.
The numbers are real; they just are not evidence about the emulator. The build is recorded
and deliberately **not** compared, because it changes on every release and would fire
constantly, drowning the two that matter.

Alongside it, every run now prints what it stands on: how many of its calls reached an
implementation rather than a placeholder. A call count is progress only to the extent the
calls were answered by something real, and reporting the total alone lets the two be
confused in the direction that flatters.

### It was demonstrated, not argued

Against PPSA28061, unchanged except for that one line:

```
  imports  47 distinct (+0), 933 calls (+0)      ->  48 distinct (+1), 935 calls (+2)
  verdict  same     nothing moved                ->  FURTHER  reached imports it could not reach before
           ! unimplemented functions now answer ok instead of unimplemented,
             so this verdict measures a settings change
```

The hack works. It is now impossible to read the result without being told so.

