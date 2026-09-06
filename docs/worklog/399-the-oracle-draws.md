# 2026-09-04 - (/loop) The oracle draws

```
phase 6 step (b) DONE - clear red, draw blue through hand-written shaders, every pixel blue
four breaks across the two steps, each landing on a different pixel
suites 133   clippy/fmt/identity clean on both repos
```

Thirty-sixth cron tick. No guest binary (checked, one command). D549 built the attachment
plumbing; this is the draw.

## What it draws

Cleared to **red**, fragment shader writes **blue**, every pixel comes back blue. The colours
differ on purpose: had both been blue, a pipeline that never bound, a vertex shader producing no
triangle and a fragment shader whose output went nowhere would *all* have passed, because the
clear alone produces the expected image. **Red is what failure looks like** - a different answer
rather than an absent one.

## Hand-written shaders, and why

`fullscreen_triangle_vertex_module` and `constant_colour_fragment_module` in `orbistoun-spirv`,
assembled instruction by instruction. **The oracle exists to check the translator, so a shader
the translator produced could not check it.** The builder is still project code, so the argument
is not that it cannot be wrong - it is that a builder wrong enough to matter would have to
produce exactly the colour asked for from a different one, which is not a failure mode a mistake
has.

A constant table rather than the usual shift-and-multiply-add: one `OpAccessChain`, no
arithmetic, and a wrong constant is visible by reading it where a wrong shift is not. The
triangle is twice the viewport, so an unwritten corner means the geometry is wrong rather than
that the test looked in the wrong place.

## Vocabulary the builder needed

No `Vertex` execution model, `Output`/`Private` storage classes, `Position`/`VertexIndex`
built-ins, `Location` decoration or `OpConstantComposite`. All published SPIR-V values, added
with the shape row the checker needs.

**One correction fell out**: `OpVariable`'s optional initialiser is an identifier and the shape
table did not check it, so a variable initialised with an undefined id passed `check()`. Fixed.

## Four breaks, four different landings

`DONT_CARE` instead of `CLEAR` fails pixel (0,0). A copy region one row short fails the **last**
row only. `cmd_draw` with zero vertices leaves red everywhere. A triangle shrunk to a quarter
fails at (3,0) **while (0,0) still passes**.

**A test sampling one corner would have missed two of the four.** Made twice by different
mistakes, which is why the plumbing was built and broken before the draw sat on it.

There is a control too - `drawing_nothing_still_answers_the_clear` - or a pipeline that somehow
drew on every path would make the draw test pass for the wrong reason.

## What it cannot say

**Nothing about the translator**; nothing translated is involved. It says the harness can put a
shader's output on the screen and read it back, which is what every later comparison rests on.
Nothing about interpolation, depth, blending or multiple attachments either.

Roadmap G12 and G11(b) rewritten (check 13).

Decision: [D550](../decisions/D550-the-oracle-draws.md).
