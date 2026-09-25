# 836. The vertex program is read from PGM_LO_ES, and Neverball's spikes are gone

**2026-09-24**: worklog 835 left Neverball's geometry fanning into a point near the screen centre,
with the next step named: measure one fanning draw instead of guessing.

## The measurement

The run's last submission is now dumped whole:

- `last-submission.txt`: the command list, plus a closing line naming the window;
- `last-module-*.spv`: the translated modules;
- `last-window.bin`: the memory window's own words. `Submission.guest_memory_base` is new and records
  where they were read from.

A scratch decoder (`decode_draws.py`, outside the tree) read every draw's three clip-space positions
out of the window at its vertex user-data offset, the way the GL context writes them: position first,
then the parameters.

Neverball's last submission has 412 draws in a 64 KiB window at `0x740001670000`. Draws 0–72 step
their vertex offset by `0x90` (144 bytes: three 48-byte vertices, two parameters). From **draw 73**
the step is `0xc0` (192 bytes: three **64-byte** vertices, three parameters). Yet the submission bound
**one** vertex module throughout. Every three-parameter draw ran the two-parameter program, which
reads each vertex 48 bytes after the last. From the second vertex onward it read parameters as
positions, which often came out as `(0, 0, 0, 0)`. That is the degenerate vertex the fans converged
on.

## The cause

The GL context switches vertex programs with `gl_hw_emit_param_count` (oops-sdk `gl_draw.c`). It
writes `SPI_SHADER_PGM_LO/HI` for **GS and ES** only (`0x88`/`0x89`, `0xC8`/`0xC9`).

orbistoun's vocabulary took the vertex program from `0x2C48`/`0x2C49`, `SPI_SHADER_PGM_LO_VS`. That is
the legacy vertex stage, which an NGG pipeline does not fetch from. It matched only because the
per-frame stage tables in both the SDK's GL and AGC draw paths point VS, GS and ES at the same code.

On this generation the NGG wave takes its program counter from **`PGM_LO_ES`**:

- Mesa `radv_shader.c:2078-2084`: `R_00B320_SPI_SHADER_PGM_LO_ES` for GFX10 and 10.3.
- `gl_draw.c`'s stage table: "on GFX10 the NGG wave takes its program counter from PGM_LO_ES",
  measured on the console.

The `packets.toml` entry had said so itself: "every entry is a hypothesis until checked against a
real submission". This was the check.

## The fix

`crates/orbistoun-gpu/data/packets.toml`: the vertex `shader_address` pair is now `0x2CC8`/`0x2CC9`
(ES). Its comment gives the history and the two citations.

New test `the_vertex_program_is_the_es_pair`: a stream pointing VS at one program and ES at another
names the ES one. It fails under the old mapping.

## What moved

Neverball, 150 s: 14 frames drawn, 15 of 15 submissions to completion, 0 refused. The last submission
now binds **two vertex programs** (4 modules, 31 vertex binds).

**The spikes are gone.** The latest frame shows the title-scene backdrop and Neverball's **coins**:
the small yellow discs, placed and in perspective across the lower right.

The level's own surfaces (floor, walls) are not visible yet. That is the next question: whether their
draws are there and land off-screen, are culled, or need depth.

The cube is pixel-identical to before: 63,293 distinct colours, the same top four.

## Gate state

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
