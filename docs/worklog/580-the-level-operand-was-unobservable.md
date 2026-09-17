# 580. The level operand was unobservable until the texture had two of them

**2026-09-15** - orbistoun-gpu-vulkan, after worklog 579

`image_sample_l` has translated since worklog 577 and had never drawn. Closing that turned out to
need a change to the harness rather than to the translation, and the reason is worth stating
because it is a shape that recurs.

## 1. A test on a one-level texture proves nothing about levels

Every texture this harness bound had **one** mip level. A sample naming level zero and a sample
naming level one both answer that level, so a device test of a levelled sample on such a texture
passes for a translation that drops the level operand entirely - which is the one claim the
instruction rests on.

That is the same failure a test comparing the implicit and explicit sampling forms would have had
(worklog 572), and the same shape as the probe that lied in worklog 565: an input that cannot
distinguish the right answer from the wrong one.

## 2. Two levels, and the coarse one is deliberately none of the others

The harness now builds a second level wherever the image is big enough for one - a one-by-one
texture has exactly one level, because halving it reaches zero, which is not an extent - and
fills it with a **fixed texel the harness chooses**, not the caller.

That last part is the point. A caller picking the coarse texel could pick one that collides with
a texel of the fine level, and then the assertion passes for a translation that ignored the
level. Fixing it in the harness is what makes "these two answers differ" a property rather than a
hope.

Three things had to move together, and each would have produced a passing-looking failure alone:

- the **barrier** covers every level, or the coarse one is left in an undefined layout and a
  sample naming it is undefined behaviour rather than an answer;
- the **copy** is one region per level, the coarse level's texel sitting immediately after the
  fine level's in the same staging buffer;
- the **sampler**'s maximum level of detail is raised, or it clamps every request to zero and a
  levelled sample reads the fine level whatever it asked for - which looks exactly like a
  translation that ignored the level.

## 3. What the test now says, and what it still cannot

The same coordinate at two levels gives two different answers, each the right one. That is the
level operand reaching the instruction, and nothing else here could have shown it.

It **cannot** say the level is the *last* address element rather than some other one. With a
two-register coordinate and a one-register level there is only one arrangement that puts a number
in the level's place, so the test passes for any translation that reads the third register as the
level. Where it sits was measured from a compiler instead (worklog 577), and the test is not a
second opinion on that.

## 4. Where the image work ends

Every image instruction the corpus contains is translated and every one has drawn on a device:
the two sampling forms, the levelled sample, the texel fetch and the store. The corpus is at
184 of 184 instructions and 12 of 14 shaders, with both remaining shaders refused by decision
rather than by a gap.

## 5. Files

- `crates/orbistoun-gpu-vulkan/src/framebuffer.rs` - the second level, its barrier, its copy,
  the sampler's ceiling, and `COARSE_TEXEL`.
- `crates/orbistoun-gpu-vulkan/tests/translated_sampling.rs` - the levelled sample, drawn.

## Next

1. `REQ-20260914T2348Z-4e71` - which register carries a buffer address, for a window the
   pipeline can derive rather than be told.
2. `REQ-20260914T1720Z-9c4a` - the payload split D688 assumes.
3. The corpus has nothing further to say about instruction breadth. What it cannot say anything
   about is whether a real shader hits either remaining refusal, and the oracle records are two
   shaders.
