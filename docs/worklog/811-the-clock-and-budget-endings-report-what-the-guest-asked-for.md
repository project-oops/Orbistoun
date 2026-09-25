# 811. The clock and call-budget endings now report what the guest asked for — and Neverball's data wall names itself: `/app0/./data`

**2026-09-24** — Neverball's first run (worklog 810) said *fs_load found nothing* for a font that sits
under `/app0/data/ttf/`, and the report said nothing about why: no syscall census, no *paths nothing
here holds*, and `ORBISTOUN_TRACE_OPENS=1` printed no list of opened paths either. The guest's file
traffic all goes through the SDK's syscall gadget, so those three reporters are the only place it is
visible, and none of them ran.

## Why: two of the five endings skipped the reporters

`report::what_the_guest_asked_for` is the one function that names every after-the-run reporter —
the syscall census, the paths wanted, the paths opened, the maps given, what the guest said. Its
callers were the fault handler, `guest_stopped` and the ordinary return. The two other ways a run
ends did not call it:

- **the call budget** (`on_budget_reached`), which is how Neverball's run ends — it plays until it has
  made its 20,000,000 calls;
- **the time limit** (`start_time_limit`), the ending of any guest that neither faults nor returns
  within its clock.

`guest_stopped`'s own comment records this exact failure twice already — reporters wired into some of
the endings and not all of them (D387, worklog 425). This is the same shape a fourth time. Both endings
now call `what_the_guest_asked_for()` before collecting the trace, as the other three do. The budget
handler runs once, after the last call the budget allows, so the allocation the reporters make is the
same trade the `collect_calls` beside it already makes.

## What the rerun shows

```
orbistoun: the guest made 361 syscalls, in this order:
    2      5  open   ...
orbistoun: the guest asked for 9 paths nothing here holds:
  /app0/./data
  /app0/.neverball  /app0/Replays  /app0/Scores  /app0/Screenshots
  /app0/capture  /app0/neverball.log  /app0/savedata  /dev/dce
orbistoun: the guest opened 2 paths:
  /app0
  /app0/eboot.bin
```

The data wall is `/app0/./data`. Neverball builds its data directory as `fs_base_dir() + "/" +
CONFIG_DATA`, and `CONFIG_DATA` is upstream's `./data`, so the path carries a `.` component. `/app0`
itself opens; `/app0/./data` does not, so orbistoun's path resolution (`orbistoun-fs::mount::resolve`)
does not collapse `.`, which a POSIX path lookup does. That is the next unit. The `/app0/*` entries
after it are the game's writable state (config, scores, replays, the log), which it expects to create
in its own package directory, as it does on hardware — `mkdir` precedes several of those opens in the
census.

Verdict `same`: this is a reporting change and the guest runs exactly as before (49 imports, 20,000,000
calls, 96% standing).

## Gate state

`crates/orbistoun-worker/src/report.rs`: two calls and their comments. No test — the two endings end
the process, and the reporters themselves are covered where they are defined; the rerun above is the
observation that they now fire. Generated docs regenerated after the run. `./bin/orbistoun check`
green, worklog index regenerated, identity scan clean. No commit.
