# Running a title, and reading the report

The point of a run is not that it worked. It is **what the guest asked for and what it got**,
which is the only thing that can be checked.

## Why the report exists

An emulator of an undocumented platform can tell you *that* a guest died and almost never
*whether an answer was right*. A function returns a number, the guest carries on or it does not,
and forty thousand frames later something is wrong. There is no specification to test against,
because the specification is the thing being reconstructed.

So a run records the calls, and comparing two runs is how a change is judged.

```bash
orbistoun-cli run <path-to-eboot.bin>      # execute; prints its own FURTHER/same/BACK verdict
orbistoun-cli report <path-to-eboot.bin>   # survey + persist a report; the delta from last time
orbistoun-cli verify <path-to-eboot.bin>   # how much of the import list the symbol db can name
```

`run` prints its own progress verdict on every call - see below. `verify` is a different,
narrower question - naming coverage, not progress - and is covered on its own in
[naming.md](naming.md).

## Honest failure, and why a stub is not free

**A stub that returns success is indistinguishable from working code until forty thousand frames
later.** Orbistoun's rule is that a function it has not implemented says so, loudly, at the call
- rather than returning zero and letting the guest carry on into damage it caused somewhere else.

That makes some titles stop *earlier* than they otherwise would. That is the intended trade: an
early stop names its cause, and a late one costs a day of bisection.

When you see a run get *less far* after a change, that is not automatically a regression. It is
what success looks like when the change replaced a lie with a refusal.

## Levels, when you want more

An ordinary run is quiet. Everything below is off unless asked for:

```bash
OOPS_LOG=debug orbistoun-cli run <title>          # decisions and resolved configuration
OOPS_LOG=trace orbistoun-cli run <title>          # per-call detail
OOPS_LOG=warn,orbistoun_loader=debug orbistoun-cli run <title>
```

`OOPS_LOG` and `RUST_LOG` both work; the first is the one the whole collection answers to.

Levels mean the same thing in every tool here: `error` is giving up, `warn` is a surprise that
did not stop the work, `info` is an action with a side effect, `debug` is decisions, `trace` is
per-item.

## The verdict, and what it is not

`run` compares this run with the previous one for the same title and prints `FURTHER`,
`same`, or `BACK`; `report` prints the same delta at more length (reached state, newly
resolved and newly unresolved imports). Both are *differential*: they know that something
changed, not that the new answer is correct.

Nothing here can tell you an answer was right. That question needs an oracle, and the oracle is
either a probe you wrote yourself ([obSCEne](https://github.com/project-oops/obSCEne)) or real
hardware reached through [Prosperous](https://github.com/project-oops/Prosperous). A commercial title
can only ever tell you it stopped.

## Where the artefacts go

Reports, traces and screenshots land under the data root - see [paths](paths.md), or run
`orbistoun-cli paths` to be told exactly where, on this machine, in this mode.
