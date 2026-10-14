# Graphics

A guest draws by building command buffers in the vendor command-stream format and submitting
them. Orbistoun decodes those buffers, translates the guest's shaders to SPIR-V, and executes
the result on a Vulkan device. The frame a guest flips is shown in the window.

## The pipeline

| Stage | Crate | Does |
|---|---|---|
| decode | `orbistoun-gpu` | walks the command packets, keeps the register file, assembles pipelines. It has no dependency on a host graphics API. |
| shaders | `orbistoun-shader`, `orbistoun-translate`, `orbistoun-spirv` | decode the vendor shader bytecode and translate it to SPIR-V |
| execution | `orbistoun-gpu-vulkan` | makes resources resident, runs dispatches and draws, and reads results back. The only crate that names a graphics API. |

A guest's geometry stage translates to a mesh shader, so a device without mesh shading refuses
a draw that needs one rather than issuing it. A command the backend cannot perform is refused
by name, so an unimplemented command is never mistaken for a rendering fault. Translated
shaders are checked by executing them on a real device and comparing the values they produce.

## In the window

While a title runs, the right side of the window shows the last frame it flipped, as large as
fits with its aspect kept, on black. Until the first frame arrives the window says the title
is starting. In the shell view the frame fills the window.

### Performance overlay

`F3` shows and hides an overlay over the running title's picture, updated about once a
second:

| Line | Shows |
|---|---|
| fps | frames flipped per second |
| submissions/s, draws/s | how many command buffers and draws those frames took |
| gpu busy | the share of the second the device was working |
| shares | each part of presenting a frame as a percentage of the second, the largest highlighted, with the remainder (the guest's own time and anything unmeasured) listed as such |

### Renderer

The window itself draws through `wgpu`, and reports which backend and adapter it got: printed
to the terminal at startup as `orbistoun: renderer: <backend> - <adapter>`, and shown in the
shell view's footer.

## What it does not handle yet

The pipeline refuses what nobody has measured rather than guessing, and every refusal names
its reason in the run report. `orbistoun-cli census --refusals` ranks the shader and backend
refusals the runs recorded by how many titles reach them: that ranking is outstanding GPU work, and each
reason is settled by an obSCEne measurement before it is implemented
([THE_LOOP.md](../THE_LOOP.md#the-capability-sub-loop)).

## Diagnostics

| Variable | Does |
|---|---|
| `ORBISTOUN_TARGET_WRITEBACK=flip\|submit` | write a drawn target back at the flip (default) or after every submission |
| `ORBISTOUN_GPU=discrete\|integrated\|<index>\|<name>` | which host GPU runs the guest: the first discrete one (default), the integrated one, a device by its `vulkaninfo --summary` index, or by part of its name (D828) |
| `ORBISTOUN_TRACE_SUBMITS=1` | print a line for every submission whose draws ran; refusals are printed regardless |
| `ORBISTOUN_PERF_DETAIL=1` | print each submission span's time once a second, beside the performance phases |
| `ORBISTOUN_PROFILE=<n>` | sample the guest's main and device threads and print where they spend their time |
| `ORBISTOUN_FLIP_TO_ALL=1` | post a flip completion to every queue |

```bash
orbistoun-cli shaders <directory> --top 10   # rank the instructions that block translation across a directory of shader binaries
```

### Profiling with Tracy

Every build carries the [Tracy](https://github.com/wolfpld/tracy) profiler's client, version
0.14.1, dormant until asked for (D829, D831). Asked for, it streams the same phases and spans as
zones on the thread that ran them, a frame at each guest flip, and each frame's time as a plot.

```bash
orbistoun-cli perf --install-tracy            # fetch the Tracy tools into the data root, once
orbistoun-cli perf <title> --seconds 30       # record a run, rank where its time went
orbistoun-cli perf <title> --open             # ... and open the recording in the Tracy viewer
orbistoun-cli bench --seconds 30              # every title in turn, ranked across them
ORBISTOUN_TRACY=1 orbistoun-cli run <title>   # just start the client, for a viewer to connect
```

`perf` keeps the recording and its report in the title's `perf` directory and reports frame times
(median, p95, p99, worst) and the zones by self time - what to make faster - and by total time -
where the time sits. A zone that hands work to another thread and waits for it, such as `execute
hand-off`, keeps only the hand-off as its own time: the report takes out the time of the zone it
waits on, which the profiler counts on the other thread too (`WAITS_ON`).
`perf`, `bench` and `ab` run a title as the window plays it - on the host clock, its submissions
carried out alongside it (`AS_PLAYED`) - unless the environment sets either. `bench` replays each
title's pad script, skips a title while something else keeps the GPU busy, and compares against
the last benchmark.

| Variable | Does |
|---|---|
| `ORBISTOUN_TRACY=1` | start the Tracy client in the worker, on `TRACY_PORT` (8086 unset) |
| `ORBISTOUN_TRACY_TOOLS=<dir>` | where the Tracy tools are, before the installed copy and `PATH` |
