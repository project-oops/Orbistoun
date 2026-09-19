# 688. The submission report reaches the run report

**2026-09-17** — the follow-on named in worklog 687. `sceAgcDriverSubmitDcb` records a
`SubmissionReport` and `last_submission_report()` exposes it, but nothing surfaced it in the
human-readable run report. This wires that last step, so the first title to reach a submission has its
command buffer counted where the reach and call counts already are (3861).

## The wire

- **A summary, not the whole report.** `SubmissionSummary { packets, register_writes, draws,
  shaders_found }` in `orbistoun-report/src/trace.rs`, a plain serde struct beside `FormatReport`. The
  translator's `SubmissionReport` carries diagnostic vectors (failures, disagreements) that are its to
  read; the run report wants the counts.
- **A `CallTrace` field.** `submission: Option<SubmissionSummary>`, `#[serde(default,
  skip_serializing_if = "Option::is_none")]` so every older trace and every run that never reached a
  submission - all of them today - stays unchanged. Three struct-literal construction sites gained
  `submission: None`; the serde-`from_str` builders needed nothing.
- **Populated in the worker.** `orbistoun-worker` gained an `orbistoun-gpu` dependency and a
  `submission_summary()` helper reading `agc_driver::last_submission_report()` into the summary,
  beside `frames: orbistoun_video::flips_accepted()`.
- **A finding.** `Gap::Submitted` (progress, not a wall, like `Gap::Captured`) and a `submitted()`
  diagnostic: *"the guest submitted a command buffer: N packets, M draws, K shader candidates"*, with
  the action *translate the shaders its registers name, then attach a backend*. Ranked by packet count
  at `Confidence::Certain` (it is the guest's own call, directly measured). `orbistoun-turn` maps it to
  a `Step::Person` - graphics work a person schedules, not a step the loop sweeps.

## Watched failing, and the negative that matters

`a_submitted_command_buffer_is_a_finding_and_an_ordinary_run_is_not`: a trace carrying a submission
produces the `Submitted` finding naming its counts; an ordinary run (`submission: None`) produces
none. The negative is the case that holds every day, since the corpus stalls before submit (D613), so
it is the one asserted first.

## A drift caught in passing

`status --check` was red on `implemented 761 → 765` and `behaviours 812 → 814`: worklog 687 added
`sceAgcDriverSubmitDcb` to `implementations()` (and its measured knowledge entry) and did not
regenerate the numbers block - the exact "generated docs drift together" lesson, and a reminder that
687 ran the completeness test but not `status --check`. Regenerated `README.md` and
`docs/PROJECT_STATUS.md`; every changed figure is this session's own implementations.

## Gate state

Workspace builds; `orbistoun-report`/`-worker`/`-turn`/`-gpu` tests pass; `clippy -D warnings` clean on
all four (a `too_many_lines` on `collect_with_fault` fixed by the `submission_summary()` extraction);
`status --check` exit 0; `./bin/orbistoun prose` exit 0; `cargo fmt --check` clean; knowledge-audit 27
pass; identity scan clean. No commit.
