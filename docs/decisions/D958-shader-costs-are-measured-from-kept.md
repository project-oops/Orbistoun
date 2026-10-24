# D958 - Shader costs are measured from kept translations

**Status:** decided
**Date:** 2026-10-24
**known_by:** measured

What a title's shader programs cost is measured from its kept translations, not by running the
title. `orbistoun-cli shader-census` makes each kept translation again from its bytes and context
with this build's translator, and records module words against guest instructions and the model
chosen. With `--compile` it has the driver compile each distinct module on up to four threads, with
no pipeline cache and outside the compile watch, and times a compute module's whole pipeline
through the layout a dispatch uses; a draw's module is timed as a shader module alone. Rows are
written as TSV, so two builds compare row by row.

**Why:** a translation that grew with the wave's width, and a driver compile of minutes, surfaced
one program at a time inside forty-minute runs of Grand Theft Auto V. The store already holds every
program the title ran with its context, so all of them are measured in minutes - its 1,084 programs
in 2 s, their compute pipelines compiled in 3.5 min - and a translator change is confirmed across
every program at once.

**Rejected:**
- Timing compiles inside a run: one program a run, serialised behind the guest.
- Reading sizes from the kept modules: they are the build that made them, not this one.
- Compiling draws' full pipelines: the store does not keep the render state a draw pipeline needs,
  and inventing it would time a pipeline no run builds.
