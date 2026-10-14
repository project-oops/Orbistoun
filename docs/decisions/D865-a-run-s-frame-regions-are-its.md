# D865 - A run's trace, frames and dumps are its own

**Status:** assumed
**Date:** 2026-10-14

**Amends:** D695 - the region an `Event::Frame` names is a full path, not a bare name.

A run writes everything it leaves behind into `runs/<title>/<run>/` under the traces directory:
the title named by the module's last two path components, the run by a `RunId` minted when the
worker prepares the run, its suffix the worker's process id. The directory holds:

- `trace.json`, the run's call trace.
- Its frame regions. The `region` an `Event::Frame` carries is the region's full path, so a log
  line names the file that holds the picture and the directory names the run that drew it.
- Its dumps: the `last-`, `refused-`, `failed-` and `drawn-NN-` submission listings with their
  guest memory windows and SPIR-V modules, the drawn-frame snapshots, `latest-drawn-frame.rgba`,
  `refused-dispatch-program.bin`, and the kept `flip-*.rgba` frames when `ORBISTOUN_FRAME_DIR`
  names no directory of its own. Each log line that names a dump names the directory.

A title's previous run is its latest that wrote a trace: run ids sort chronologically, and a run
that died before writing one is passed over, so it is not reported as a measurement (D470). A
reading across titles - the census, the worklist, the questions - takes each title's latest trace
once.

The shim reads a region only within the traces directory it was given: a path outside it, or one
with a parent component, is refused as a bare name with a separator was before.

A title keeps its last four runs. When a run starts, the title's older run directories are
removed; a run whose directory goes while it still writes makes it again.

**Why:** every run on the machine shares the traces directory, and runs in several sessions draw
at once. GTA V (PPSA04263) presented its first 3840x2160 frame as `frame-1000000.bin`, and
minutes later the same file held a 1920x1080 Neverball frame from another session's run: the
picture was lost, and the descriptor that named it named Neverball's. A title's trace was one
file, so two runs of one title wrote over each other's in the same way.

**Known limits:**
- Two runs of one title at once: the one that reads its result after the other wrote a trace may
  take the other's as its own, since the shim knows the title but not the run id the worker
  minted.

**Rejected:**
- The title and run in a bare file name, beside the trace: the traces directory fills with every
  run's ring, and nothing groups or prunes them.
- A directory per title alone: two runs of one title at once still overwrite each other.
