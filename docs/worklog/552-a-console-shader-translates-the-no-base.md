# 552. A console shader translates: the no-base code was wrong and the stage was never passed

**2026-09-14** - orbistoun-translate, orbistoun-gpu and orbistoun-shader, after worklog 551

The GL cube's untextured pixel shader - written by oops-sdk, run on a retail console on
firmware 12.40, and captured whole in oracle record A - now goes through the submission
pipeline and comes out as an 18,210-word SPIR-V module. `spirv-val` accepts it. That is the
first shader a console ran that this project has turned into a module something other than
itself will vouch for.

Two faults stood between it and that, and both had been mistaken for questions about the
hardware.

## 1. The no-base code was 0x7d, and the translator expected 0x7f

Since worklog 545, both pixel shaders stopped at their canary store: *flat access has a base
this translator does not understand*. The scalar-base field held `0x7d`, the translator's
`FLAT_NO_BASE` was `exec_hi` - the top of the field, `0x7f` - and the difference was filed as
a question for the console: does `0x7d` mean "no base", or "a base of zero"? It is neither.

The reference assembler settles it in one line. For this target, `llvm-mc` assembles

```
global_store_dword v[8:9], v10, off offset:4
```

to `0xdc708004 0x007d0a08` - *exactly* the word the console's shader carries - and
disassembles those bytes back to the same instruction. `0x7d` is the no-base form. `0x7f` is
not what the assembler emits or what any shader contains.

**Nothing caught it because the constant and every test that used it were written together.**
The tests encoded `0x7f`, the translator accepted `0x7f`, and the pair agreed with each other
and with no shader that exists. The one real stream that reached the code was refused, and
the refusal was read as evidence about the hardware rather than about us. That is the exact
failure mode the differential fixtures exist to prevent, and no fixture had ever contained a
flat access with no base.

One does now. `primitive.s` gained the cube's own store, so the bytes are committed, the
decoder is checked against them, and the next disagreement is a test failure instead of a
refusal that looks like a hardware mystery. The constant is `null`, the name our operand
table gives code `0x7d`, with the measurement written beside it.

The differential test caught something real on the way: the reference prints that operand as
`off` and our table names the code `null` globally. Both are right - one names per field, the
other names codes - so the comparison offers `null` wherever the reference said `off`, the
same collapse it already makes for the export targets.

`REQ-20260914T1402Z-b6d8` on the obSCEne bus has an addendum saying all this. What remains of
it is a console-side confirmation that the canary lands, which is worth having and blocks
nothing.

## 2. The pipeline had the stage and never used it

Past the store, both pixel shaders stopped at their first interpolation: *this module has none
for that attribute - translate at the fragment stage*. The pipeline was translating every
shader as a compute dispatch, while holding the stage the register vocabulary had already
attributed it to. The candidate's own comment said *reported, never dispatched on*.

Now it dispatches. `translate_staged` takes the stage and, for anything that is not compute,
uses the wavefront model - the only one with fragment inputs and a colour output - and says so
through the same warning the automatic fidelity resolution already uses, because the cost is
real and a caller should not have to infer it.

Two things that had to come with it:

- **The cache is keyed by content and stage**, not content alone. The same instructions
  translated for a fragment stage and for a compute dispatch are different modules, and a
  cache that ignored the stage would serve whichever was translated first.
- **A vertex shader is still attempted as compute**, deliberately, and the reason is written
  where the mapping is: there is no host vertex stage until the mesh stage lands (D688), and
  refusing on that basis would replace the report's instruction-level answer - the measurement
  the worklist is built from - with a policy. Every vertex program reaches the geometry-engine
  message first anyway, which is blocked and points at the same decision.

## 3. What the oracle says now

| | record A, untextured | record B, textured |
|---|---|---|
| vertex program | stops at `0x14`, `s_sendmsg` (D688) | same |
| pixel shader | **translates**, 18,210 words | stops at `0x84`, `image_sample_lz` |

Record B's remaining blocker is the image subsystem, which is where worklog 549 left it.

The test no longer only prints this. Which shaders translate is pinned per record, including
the vertex program's refusal, so the next movement is a test failure rather than a line of
output nobody was watching - this measurement has now moved four times and every move was
noticed by reading. The reasons stay printed, because a reason is prose and pinning prose
makes a test fail when somebody improves a sentence.

## 4. An independent validator, not our own opinion

The test writes each translated module to `target/spirv/`, which is where
`tools/validate-spirv.sh` already points `spirv-val`. Until now the only modules there came
from a hand-written example. `spirv-val` on `oracle-agc-gl-cube-fw1240-a.spv` - 72,840 bytes,
translated from a shader a console ran - exits zero with nothing to say.

That is the strongest statement available about this module short of drawing with it: the
emitter's own tests check structure, and a crate asserting it likes its own bytes proves
nothing.

## 5. Files

- `crates/orbistoun-translate/src/model.rs` - `FLAT_NO_BASE`, measured.
- `crates/orbistoun-translate/src/lib.rs` - `translate_staged`; `translate` is now it, at
  compute.
- `crates/orbistoun-translate/src/wavefront.rs` - the stage-less entry point deleted, nothing
  called it.
- `crates/orbistoun-translate/tests/{execute,agreement}.rs` - the measured code, named once
  per file instead of written inline four times.
- `crates/orbistoun-gpu/src/pipeline.rs` - `host_stage`, the stage into `prepare`, the cache
  salt.
- `crates/orbistoun-gpu/tests/oracle_gl_cube.rs` - the outcomes pinned, the module emitted.
- `crates/orbistoun-shader/tests/differential.rs` - `off` is `null`.
- `tools/shader-fixtures/primitive.s` and the fixture it regenerates.

## Next

1. The image subsystem - record B's pixel shader is the only thing in the corpus waiting on
   it, and it is the last blocker on a textured frame.
2. The mesh stage (D688), for the vertex programs.
3. Draw with the translated module. It validates; whether it produces the console's pixels is
   a different question, and the framebuffer oracle in `orbistoun-gpu-vulkan` is where it
   would be asked.
