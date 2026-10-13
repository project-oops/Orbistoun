# D829 - A tracy build streams the perf spans to the Tracy profiler

**Status:** assumed
**Date:** 2026-10-13

A build with the `tracy` cargo feature (`orbistoun-cli` or `orbistoun-gui`, forwarded to the
worker and `orbistoun-gpu`) starts the Tracy profiler's client, version 0.14.1 by
`tracy-client` 0.19, and emits every perf phase and span as a Tracy zone on the thread that ran
it, named as the run report names it, with a frame mark at each guest flip. Each label is
written once, in a table both the report's names and the zones are made from. The client
records only while a viewer or `tracy-capture` is connected, and is reachable over the network,
so a run on another machine can be profiled from this one. Every default build is without it.

The client's crash handler, system tracing, context-switch tracing and sampling are off.

**Why:** finding a cost meant timing a span, then adding temporary timers inside it and
rebuilding, two or three times a cost. Zones nested by thread show the whole tree at once, with
each frame's time beside it, and the spans already marked the places worth timing. Tracy is the
profiler emulators and engines use for this; writing one would be building it again.

The crash handler installs an exception handler of its own, beside the one the guest-page guards
(D717, D720, D824) depend on answering first. Sampling walks stacks through ETW, which refuses
guest threads' stacks, as samply's did, and system tracing needs elevation.

**Rejected:**
- Text logs at every span: formatting and writing a line per span costs more than most spans
  measure, and changes the program it observes.
- A profiler of our own: Tracy already records zones, frames and GPU time, and its capture
  tools serve bulk runs.
- On in every build: the client opens a network port and costs a load per span even while
  nothing is connected.
