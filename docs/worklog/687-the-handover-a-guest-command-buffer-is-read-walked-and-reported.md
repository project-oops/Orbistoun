# 687. The handover: `sceAgcDriverSubmitDcb` reads a guest command buffer, walks it, and reports

**2026-09-17** — inbox `-3861`: nothing in the tree read a guest command buffer at run time. A guest
builds one with `libSceAgc` (46 of its 57 names implemented) and hands it over with
`sceAgcDriverSubmitDcb`, and that function was declared and unimplemented, so `walk` and `pipeline`
were reached only from tests and `orbistoun-cli`. Without it, attaching a backend to the run path
(`-36c0`) buys nothing, because nothing would ever be handed to it. This is the handover.

It was blocked on the descriptor layout, filed as obSCEne `-1c97` and **now resolved**: the call reads
a 16-byte descriptor `{gpu_addr: u64, size_dwords: u32, flags: u8, pad}` (measured on FW 12.40, sweep
`20260917-124503`).

## What `submit_dcb` does

`crates/orbistoun-gpu/src/agc_driver.rs`: reads the descriptor at `arg0`, takes the command buffer's
address and its `size_dwords * 4` byte length, walks it through `orbistoun_gpu::walk` and the pipeline,
and records a `SubmissionReport` (packets, register writes, draws, shader candidates) that
`last_submission_report()` exposes for the run report. Returns `0x0`, the `rc-submit` obSCEne measured
on every `166-agc/driver-submit-*` check (sweep `20260916-223136`).

**No backend is attached, so it reports rather than renders** — the count of packets, draws and shader
candidates that says where translation effort goes (3861), not a frame.

## The two safety properties, and why they hold

- **It never faults on the guest's account.** A null or absurd descriptor (`gpu_addr == 0`,
  `size_dwords == 0`, or a size past a 16 MiB ceiling) records nothing and still returns success, as
  the console's does. An arbitrary or truncated *content* is the walker's problem, which
  `pipeline`'s tests already require it to survive - a stream it cannot decode yields an empty command
  list and a report saying so.
- **A submit reads only the one range the descriptor named.** The command buffer is CPU-side memory
  the guest built and owns (identity mapping, D014); the sole raw guest read is of that range, into a
  `Vec`. A `SubmittedBuffer` then serves the pipeline from that `Vec`, so a shader **GPU** address the
  register writes point at falls outside it and reads back `None` - reported as unresolved (the open
  half of D101), never dereferenced. Resolving GPU addresses is a backend's job.

## Verification

`a_submitted_command_buffer_is_reported_and_a_bad_one_is_survived` (in-file): a two-packet buffer
walks into a report of `packets == 2` with its context-register writes extracted; a null descriptor
and a buffer of `0xff` bytes both return success without faulting. The knowledge entry moved from
`libSceAgc.toml` (misfiled, and stale - "nothing established") to `libSceAgcDriver.toml`, recording
the implementation, the `-1c97` descriptor and the measured return, `known_by = "measured"`.

## What remains: the run-report display

The report is produced, recorded and exposed by `last_submission_report()`, but not yet surfaced in
the human-readable run report. That wire - a `submission` field on `CallTrace` (19 construction sites,
a new `orbistoun-worker` → `orbistoun-gpu` dependency) and a `diagnose` finding - is a separate unit,
and one nothing exercises today because the corpus still stalls before submit (PPSA03416 blocks in
`sceKernelWaitEqueue` on an unposted driver queue, D613). Spawned as a follow-on task.

## Gate state

Workspace builds; `orbistoun-gpu` tests all pass (`agc_driver` submit test included);
`every_implemented_function_is_written_down` passes; knowledge-audit 27 pass; `clippy -p
orbistoun-gpu --all-targets -D warnings` clean; `./bin/orbistoun prose` exit 0; `cargo fmt --check`
clean; identity scan clean. No commit.
