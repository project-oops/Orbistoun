# 576. The dimensionality was in the instruction all along, and nothing was reading it

**2026-09-15** - orbistoun-translate and orbistoun-gpu-vulkan, after worklog 575

Worklog 575 left one instruction, `image_sample_l`, and said it needed a hardware probe because
its level sits among address registers "whose position depends on dimensionality held in the
descriptor".

**That sentence is wrong**, and finding out why turned up a defect in the three image
instructions that already shipped.

## 1. The dimensionality is an instruction field

The probe corpus has said so since the day it was written. Every line in
`tools/shader-fixtures/probes/image.s` carries `dim:SQ_RSRC_IMG_2D`, and the file's own comment
explains why the solver ignores it:

> `dim:SQ_RSRC_IMG_2D` is a modifier printed as a symbolic name, like MTBUF's `format:[...]`,
> and is skipped for the same reason: the field exists, its spelling is not a number, and
> inventing a code for it is what this repository refuses to do.

So the field is in the instruction, it is skipped on purpose, and **nothing downstream can see
it**. The claim about the descriptor was written from the shape of the problem rather than from
the probe file that answers it.

## 2. What that cost

Every image translation here reads **two** coordinate registers. That is right for a
two-dimensional image and wrong for every other kind: a three-dimensional coordinate is three
registers, and reading two of it samples a place the guest never named.

Nothing checked, because nothing could. The three instructions translated in worklogs 568, 572,
573 and 575 all carried that assumption, unstated and unenforced. A frame drawn from it would
have looked entirely plausible, which is the class of fault this project spends most of its
effort on.

## 3. Measured, in the toolchain that was already there

One instruction assembled at each of the eight dimensionalities, differenced:

| dimensionality | first byte | code |
|---|---|---|
| 1D | `0x00` | 0 |
| 2D | `0x08` | 1 |
| 3D | `0x10` | 2 |
| cube | `0x18` | 3 |
| 1D array | `0x20` | 4 |
| 2D array | `0x28` | 5 |
| 2D multi-sampled | `0x30` | 6 |
| 2D multi-sampled array | `0x38` | 7 |

**Every other byte of the eight is identical**, which is the proof the field is those three bits
from bit three and nothing else. No transcription, no reference consulted, no guess.

The assembler gives a second fact for free: it refuses an address operand whose register count
does not match the dimensionality. `image_sample_lz` with two dimensions takes two address
registers; `image_sample_l` takes three. So the level **is** one extra address register, and the
count is measured rather than assumed. Its position among the three is still convention, and that
is the part `image_sample_l` still waits on - a much smaller question than the one recorded
before.

## 4. The guard, and it caught its author first

Every image translation now reads the field and refuses anything but two dimensions.

The first thing it refused was this session's own hand-assembled test shaders, which had never
set the field - so they said one-dimensional and were translated as two. The tests were wrong in
exactly the way the guard exists to catch, and they were wrong the whole time the instructions
were being called translated.

The negative test runs all seven other codes. That is the rule CLAUDE.md states and this is what
it is for: *a guard is not finished until somebody has made it fail.*

The census is unchanged at **183 of 184** - every image instruction in the fixture corpus is
two-dimensional, so the guard costs nothing on real material and refuses what it should.

## 5. The lesson, which is not about images

The claim in worklog 575 was not measured, not cited, and not marked as an assumption. It read
like a fact and it had one source: how the problem felt. The file that contradicted it was in the
repository, written months earlier, saying the opposite in plain words.

**Before recording something as blocked on a measurement, read what the probes already say.** The
roadmap makes the same point about generators, having been wrong four times; this is the same
mistake with a different instrument.

## 6. Files

- `crates/orbistoun-translate/src/model.rs` - `IMAGE_DIMENSION`, `IMAGE_DIMENSION_2D`, and the
  refusal.
- `crates/orbistoun-gpu-vulkan/tests/translated_sampling.rs` - the dimensionality set in the
  hand-assembled shaders, and the guard watched failing on all seven other codes.
- `crates/orbistoun-gpu-vulkan/tests/storage_image.rs` - the same, for the store.
- `docs/roadmap/015-phase-6-s-contents-built-ahead-of-it.md` - G7, corrected.

## Next

1. `image_sample_l`: the level is one extra address register, measured. Where it sits among the
   three is convention, and the published instruction set for this generation states the address
   order - the same source that settled the division helpers (D143, D144).
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both on the obSCEne bus.
