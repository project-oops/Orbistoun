# 550. The census stopped claiming shaders complete that do not translate

**2026-09-14** - orbistoun-shader, orbistoun-cli and orbistoun-gpu, after worklog 549, plus
D688

Worklog 545 §4 recorded a false report and left it: the shader census said `2 of 6 complete`
for the GL cube corpus while the submission pipeline translated **none** of the same six
shaders. Both numbers were computed correctly. They were answers to different questions, and
only one of them was the question the line appeared to ask.

The census is now the same question the pipeline asks, and the answer is `0 of 6 translate`.

## 1. Why the two disagreed

"Complete" meant *every opcode in this shader is on the translator's supported list*. A
translation refuses for more reasons than an opcode:

- an operand the model has no slot for - `m0`, which cost worklog 545 its first fix
- a memory base nobody has measured - the `null` scalar base both pixel shaders still stop at
- a stage mismatch - an export has nowhere to go in a compute dispatch

None is visible to a census keyed on opcode numbers, so all three were invisible. The two
untextured pixel shaders had every opcode supported and stopped at instruction ten.

## 2. What changed

`ShaderSummary` gains `translated: Option<bool>` - what an actual translation said, or
`None` where nobody ran one. `is_complete` prefers the verdict and falls back to the opcode
estimate, and `report::summary` **says which it is showing**: `0 of 6 translate`, or
`0 of 13 have every opcode supported; no translation was attempted`. The estimate is still a
useful thing to report; presenting it as a verdict was the fault.

`observe` keeps its signature and means "no translation attempted"; `observe_translated`
carries the verdict. Only the CLI passes one, which is right - the unit tests here are decode
censuses with no translator in the picture at all.

## 3. Which stage a corpus shader belongs to

A directory of binaries says nothing about stages, and the stage is not cosmetic: judging a
pixel shader as a compute dispatch refuses it at its export, for a reason about the question
rather than about the shader. So the CLI asks **is there a stage this translates at**, trying
compute and fragment, at wavefront fidelity because that is the level that is correct
unconditionally. `translate_for` already existed and only tests used it.

That is the honest form of the question a corpus can answer. It costs two attempts for a
shader that fails everywhere and one for a shader that works.

## 4. A regression that had not happened

The first run under the new definition printed `BACK ... (-2)`. Nothing had regressed: the
measure had changed. A delta between a count of shaders whose opcodes are all supported and a
count of shaders that translate names a movement neither run observed, which is the same rule
this project already states for diagnostics - a message naming a cause must come from the
branch that determined it.

So the stored summary now records `attempted`, whether its `complete` came from translations,
and `movement` refuses to compare across a change of basis: verdict `FirstRun`, no deltas.
A record written before the field defaults to `false`, which is exactly what it measured.

The guard was watched rejecting something twice: a unit test that builds both bases, and the
real stored record from the previous run, which made the live census print `first look at this
corpus - nothing to compare against yet` instead of a regression.

`SubmissionReport::summary` in orbistoun-gpu declares `attempted: true` - a submission has
always counted shaders a translator ran over, so it was already comparable with the corpus
runs that now do the same.

## 5. D688: what a primitive shader is

`s_sendmsg` was blocked "waiting on a decision", which is a fair thing to record once and a
poor thing to leave. The decision is made and recorded as **assumed**: an NGG primitive shader
translates to a **mesh shader**.

The correspondence is not an analogy. A mesh shader's first act is to declare how many
vertices and primitives its workgroup will emit, then it writes primitive indices and
per-vertex outputs - which is `MSG_GS_ALLOC_REQ`, then `exp prim`, then `exp pos`/`exp param`,
in that order, in one workgroup, exactly as the console-run shader does it.

The cheaper option - translate the per-vertex half as a vertex shader and drop the primitive
half - is refused although it would draw this cube correctly today. NGG exists so a shader can
cull primitives before they cost anything, so a compiled shader from a real title exports
fewer primitives than it was handed, decided at runtime. Under that option such a shader
translates without complaint and draws what the guest culled: silent, frame-dependent, and the
failure this project refuses in every stub. D028 covers the rest - a shortcut that constrains
the design is never worth the time it saves, and there is no deadline.

One thing the decision records as **not known**: the allocation request's payload. Our shader
writes `m0 = 0x1003` for one primitive of three vertices, which fits a vertex count in the low
half and a primitive count above it and does not separate that from several other splits. One
sample. The way to settle it is a shader asking for a different pair and a look at which bits
move.

`s_sendmsg` stays blocked, with a destination instead of an open question.

## 6. What the numbers say now

| | |
|---|---|
| shaders that translate | 0 of 6 |
| instructions translatable by name | 196 of 200 |
| blockers, ordinary tier | none |
| blockers, waiting on a subsystem | `s_sendmsg`, `image_sample_lz` |

The corpus census and the submission pipeline now agree, which they did not yesterday, and
both say the same thing: everything the GL cube runs is named and decoded, and the three
things still refusing are a measurement (`REQ-20260914T1402Z-b6d8`), a subsystem the decision
above names, and an image subsystem.

## Files

- `crates/orbistoun-shader/src/coverage.rs` - the verdict field, `observe_translated`, the
  `attempted` basis on `Summary`, the refusal to compare across it, and its test.
- `crates/orbistoun-shader/src/report.rs` - the line says which question it answered; a test
  for each basis.
- `crates/orbistoun-shader/tests/differential.rs` - the end-to-end worklist test asserts the
  labelled wording.
- `crates/orbistoun-cli/src/main.rs` - `translates`, which asks the translator per shader.
- `crates/orbistoun-gpu/src/pipeline.rs` - a submission declares its basis.
- `crates/orbistoun-translate/src/model.rs` - the blocked reason points at D688.
- `docs/decisions/D688-an-ngg-primitive-shader-translates-to-a.md`.

## A note on the decision index

`CLAUDE.md` says never to hand-edit `docs/DECISIONS.md` because `tools/split-decisions.sh`
generates it. That script is not in the tree, and the six decisions filed today all have
hand-added rows. The row for D688 was added the same way. Worth someone deciding which is
true - the instruction or the tree - because right now a reader following the instruction
cannot index a decision at all.

## Next

1. `REQ-20260914T1402Z-b6d8`, the flat base: both pixel shaders stop there and it is the only
   blocker that is neither a subsystem nor a decision.
2. The mesh stage, when the emitter is ready for an execution model it does not have.
3. The allocation-request payload probe, which is cheap and settles an `assumed`.
