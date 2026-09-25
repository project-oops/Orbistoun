# 818. The backend's refusals are tallied by reason and the run report says why a shader did not translate — Neverball's 450 refused draws are one untranslated fragment shader, stopped at one unnamed opcode

**2026-09-24** — worklog 817 ended with Neverball's first draw submission reaching the backend and
**450 of 453 commands refused**. A count names no work. Two pieces of the answer existed and were thrown
away before a reader saw them.

## The backend's refusals, by reason

Every `BackendError::Unsupported` carries the name of what it refused; `orbistoun_gpu::render::drive`
counted them and dropped the names. `FrameOutcome` now carries `refusals` — each reason with its count,
most frequent first, ties broken by name so two drives of one submission read identically — the worker's
`RenderOutcome` carries it through, and the render log prints one line per reason. The existing
refusal test pins the tally (`("everything", 2)`).

```
orbistoun: a submission reached the ... backend: 3 command(s) driven, 450 refused, in 137 ms
  refused   450  Draw with no vertex and fragment shaders bound
```

One reason, all 450: every draw arrives with nothing bound.

## Why nothing is bound: the shader outcome, which the run report never printed

`SubmissionReport` already kept `shaders_translated` and `failures` — *every shader that did not, and
why* — but the persisted `SubmissionSummary` carried only candidates and address resolution, so the
report said "2 shader candidates" and nothing else. The summary now carries `shaders_translated` and
`shader_failures` (as `stage at address: reason`, `#[serde(default)]` so older traces still load), and
the `Submitted` finding lists them. It lost `Copy` to do so; nothing relied on it. The diagnose test
asserts both new evidence lines; the round-trip test carries a failure through serialisation.

The two baselines now say different things:

```
cube:       2 of 2 shader candidates translated to a module a backend can bind
            -> backend: 16 command(s) driven, 0 refused, frame 1920x1080
Neverball:  1 of 2 shader candidates translated
            did not translate: fragment at 0x740001656000: ... instruction at 0x84 cannot be
            translated: this target has no recorded name for that opcode
```

The cube's shaders translate and its whole draw submission runs on the backend. Neverball's vertex
shader translates; its **fragment shader** — a variant the cube never uses, at `payload-va + 0x6000` in
the GL context's shader payload — stops at the instruction at byte `0x84`, whose opcode orbistoun's
instruction vocabulary has no name for. With no fragment module, the pipeline binds neither stage and
every one of the 450 draws is refused for it. **One opcode stands between Neverball's first frame's
draws and the backend**, and naming and translating it — from the SDK's own shader source, which is ours
— is the next unit.

## Gate state

`crates/orbistoun-gpu/src/render.rs` (`refusals`, test), `crates/orbistoun-worker/src/render.rs` (carried
and printed), `crates/orbistoun-report/src/trace.rs` (`shaders_translated`, `shader_failures`, `Copy`
dropped, round-trip test), `crates/orbistoun-report/src/diagnose.rs` (the evidence lines, test),
`crates/orbistoun-worker/src/report.rs` (filled from the report). `orbistoun-report` 102 and the render
tests pass. `./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
