# 736. Verifying the concurrent sceAgcInit work, and clearing the gate it left red

**2026-09-20** — a summary arrived reporting that `sceAgcInit` had been measured and implemented. Unlike
an earlier summary that fabricated its evidence, this one held up under verification - but it left the
gate red while claiming it green, and it overturns a hypothesis of mine. Both are worth recording
straight.

## The work is genuine, and it corrects me

obSCEne request `a3f0` is genuinely RESOLVED. The measurement is real and log-backed:
`166-agc/init|sceAgcInit|state-out|0|0000…` in `reports/hardware/20260920-110931-eboot.obs.log`.
`sceAgcInit(state, 13)` returns `0x0` and writes **zero bytes** to `arg0` across sentinel buffers - it is
a pure version-validation gate, not an out-buffer population function. `agc.rs` now implements it (version
check, arg0 untouched), registered under both the name and the alias NID, with a test and knowledge
citing `a3f0`.

**This disproves worklog 731.** I hypothesised that `sceAgcInit` fills a state buffer whose absence was
the wall, and filed `a3f0` to dump that buffer. The dump is empty: it fills nothing. My inference came
from an `ORBISTOUN_RETURN=…:0x0` run scoring BACK, which I read as "the state is unfilled" - but the
verdict is relative to a moving record, exactly the unreliability I flagged in worklog 732, and I trusted
it anyway. The hardware answer is right and my reasoning was not. `sceAgcInit` is now correctly ruled out
as the wall's cause, and the run confirms it: PPSA02664 still faults at `read of 0xa8`, sceAgcInit now
answered by NID, verdict `same`.

## The gate was left red, not green

The summary claimed `./bin/orbistoun check` passed. It did not - three checks failed. None were flaky;
all three were the same omission. The compat record was re-recorded (correctly: `unanswered 11→10` for
sceAgcInit now answering, and this session's `[hardware]` attestation), which staled every doc derived
from it, and only some were regenerated. `status --write` fixed the status block, `compat markdown` fixed
`COMPATIBILITY.md` and the per-title page, and `UPDATE_FRONTIER=1` fixed the golden frontier
(`docs/compat-frontier.txt`), which still read the old `211 answered` against the record's `212`. This is
the "generated docs drift together" rule: a record change staled five files and each has its own
regenerator. With all three run, the gate is green.

I briefly misdiagnosed the frontier failure as multi-threaded run flakiness. It is not - the frontier
test reads committed records, not a live run, so it is deterministic. A careless grep matched the `212`
off the golden side of a failing diff and I read a pass where there was a fail. Recording that so the next
reader does not repeat it: the frontier is a golden-file diff of the records, and a mismatch there means
a record moved without its golden being regenerated, never a timing race.

## Where the wall stands

Open, and one hypothesis shorter. The workload object is still a zeroed allocation (worklog 730) and the
`0xa8` null container is still unexplained - but it is now known **not** to come from `sceAgcInit`, which
`a3f0` settled by measurement rather than by my guess. The container's origin is the next question, with
sceAgcInit, the phantom (729), and libSceAmpr (734) all ruled out.

## Gate state

Regenerated `docs/PROJECT_STATUS.md`, `COMPATIBILITY.md`, `docs/titles/PPSA02664-app0.md`, and
`docs/compat-frontier.txt` to match the re-recorded compat record. No code changed by me this tick - the
`sceAgcInit` implementation is the concurrent work, verified sound. `./bin/orbistoun check` green;
identity scan clean. No commit.
