# Inspecting a run

Every run records the calls the guest made into the operating system and its libraries. A
trace is binary and indexed: each event carries a global sequence number and the guest return
address it came from. This page covers the ways to read what a run did.

## In the window

When a run ends, the detail panel shows its calls (see
[running a title](running.md#the-run-result) for the whole result):

- last calls before the fault - the final calls, each as `name(first argument)`.
  Shown when the run faulted.
- what it asked for - every import the guest called and how many times.
- the stack line - how many calls arrived, and how many of them on a misaligned stack. It is
  shown when clean too, so a clean line is distinguishable from a missing one.

## Trace logging

```bash
OOPS_LOG=trace orbistoun-cli run <title>/eboot.bin     # every call, as it happens
```

Diagnostics that list one kind of event at the end of a run, each off by default:

| Variable | Lists |
|---|---|
| `ORBISTOUN_TRACE_CALLS` | the opening calls in order, with the address each was made from |
| `ORBISTOUN_TRACE_OPENS` | every path the guest opened successfully |
| `ORBISTOUN_TRACE_FORMAT` | every string the guest formatted |
| `ORBISTOUN_TRACE_MAPS` | every mapping the guest was given, in order |
| `ORBISTOUN_FINDINGS` | how many ranked findings to print in full |
| `ORBISTOUN_DUMP` | arguments for the named imports, even ones something implements |

`orbistoun-cli env` lists every variable with its current value.

## How each answer is known

Every recorded behaviour carries `known_by`, which says where the answer a guest receives
comes from:

| `known_by` | Means |
|---|---|
| `published` | documented behaviour: FreeBSD, POSIX, a published standard |
| `measured` | established on real hardware by a conformance probe |
| `guest-observed` | inferred from what guest code does with the answer |
| `assumed` | written down without the evidence to settle it; each is a question a probe can answer |

```bash
orbistoun-cli knows <name-or-fragment>   # what is known about a function, and how
orbistoun-cli knows                      # a summary
orbistoun-cli questions --top 20         # the open assumptions, ranked by how often guests call them
```

## Across runs

```bash
orbistoun-cli worklist                   # what to implement next, totalled from every run's trace
orbistoun-cli worklist --static-gap      # the same from static import lists, grouped by source
orbistoun-cli turn <title>/eboot.bin     # run, rank the findings, and take every mechanical step
```

`turn` sweeps a call's arguments, arms watchpoints and asks the other diagnostics, and stops
at each step that needs a person with a sentence saying why. [THE_LOOP.md](../THE_LOOP.md)
describes the whole turn.
