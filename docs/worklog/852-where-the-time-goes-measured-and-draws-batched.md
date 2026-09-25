# 852. Where the time goes, measured, and draws batched

**2026-09-25**. Neverball goes from 9 to 11 fps, with its pictures and gl1-probe's results unchanged.

**Measuring, kept rather than thrown away** (the user asked for timers behind a switch):

- `perf::Span`, 16 finer spans of a submission, measured only under `ORBISTOUN_PERF_DETAIL=1` and
  printed once a second beside the phases. Off, each span is one relaxed load.
- `ORBISTOUN_PROFILE=1` (or a number of lines to show) is a sampling profiler (`worker::profile`).
  Every millisecond it suspends the guest's main thread and the device thread and reads the
  instruction pointer and the top of the stack. Samples are counted:
  - by offset, in the guest's image;
  - by module and offset, in host code;
  - by the first return address into our executable, for waits in system libraries.

  Nothing allocates or locks while a thread is suspended.
- Symbolised through `dbghelp` against the release PDB (a scratch script).

**What that showed:**

- Of the "guest" remainder, only 10-18% of the main thread is guest code. ~33% is waiting on the
  device thread, which is itself idle ~70% of the time. The rest is spread thinly:
  - the thunk ~5%;
  - write-watch queries ~6%;
  - heap allocation ~3%;
  - stderr ~2%.
- Clears were ~13 ms a frame: `GuestCp::fill` wrote 4 bytes a loop. It now fills words (106 to 19
  ms/s).
- Device work was ~1 us per draw, ~17k draws a frame.

**D718, draw batching:** consecutive draws sharing their state are one mesh dispatch, one workgroup
per draw, each reading its own draw data at binding 5. Order is kept by the specification's mesh
primitive ordering. It was verified with gl1-probe (0 verdict and 0 pixel differences) and with the
presented frames.

Also fixed: `report::host_module_of`'s non-Windows stub carried both `cfg(not(windows))` and
`cfg(windows)`, so it never compiled anywhere.
