# D487 - A run is not reproducible, so the progress verdict needs a noise floor

**measured** - 2026-09-03 (seven runs of PPSA02664 on identical code)

`FURTHER` is the only measure of progress this project has, and CLAUDE.md says so: *"that is
the only measure of progress this project has, and it is the one to optimise."* This records
that **it is not reproducible at the granularity it currently reports.**

## What was measured

Seven runs of PPSA02664, same binary, same limit, nothing changed between them:

```text
imports  68 distinct, 10905 calls
imports  69 distinct, 10902 calls
imports  69 distinct, 10902 calls
imports  68 distinct, 10905 calls
```

Two states, alternating without pattern. The verdict follows them:

```text
verdict  FURTHER  executed code it could not reach before
verdict  BACK     reaching less of the interface than it did
verdict  same     nothing moved
```

**All three verdicts, on identical input.** The fault address is stable at `0x7fff0001` every
time, so it is not the crash that varies - it is the path taken before it.

## It nearly produced a false claim, immediately

The run that first exercised the linked title modules reported `FURTHER +1 distinct import`,
and that was written down as the result. It was noise: a later run of the same code reported
`BACK -1`. The wall had not moved; the measurement had.

This is principle 3's *"an intervention that moves a wall is not a diagnosis"* one level up -
an intervention that appears to move a wall, where the wall was never still.

## What an interleaved comparison does show

Three runs of each path, alternated rather than batched (which is the only way to compare on a
machine whose runs vary):

| | calls | distinct imports |
|---|---|---|
| single-module | 10884, 10887, 10884 | 69 every time |
| title modules linked | 10902, 10905, 10902, 10905 | 68 or 69 |

**+18 calls, consistently, against a ±3 spread.** That is outside the noise and is a real
effect of linking the title's own modules. The distinct-import count shows no gain at all, and
sometimes one fewer.

So the two numbers disagree about the same change, and they disagree because they measure
different things: the guest **does measurably more work** and does **not** reach a wider part
of the platform interface. The headline verdict keys on the second.

## What follows

- **A delta of ±1 distinct import is not a signal.** Any past `FURTHER` resting on one import
  is suspect, and future ones must clear a measured floor rather than a difference.
- **The call count is the more sensitive instrument here** and is not what the verdict uses.
  It resolved an effect the verdict called `same`.
- **The cause is not established.** The guest spawns threads (`015-sync/thread-churn` in the
  probe's vocabulary, and this title spawns its own), so a scheduling race is the leading
  candidate. It is a candidate, not a finding: nothing here isolates it.

## Not fixed here, and why

The obvious repair - repeat each run N times and report the spread - changes what `run` costs
and what its report means, and doing it while the cause is unknown would bake the noise in as
though it were a property of the emulator rather than a bug in it. **Diagnosing the
nondeterminism is the better first move**, and this decision exists so the next reader does not
trust a one-import verdict in the meantime.
