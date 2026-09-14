# 561. The window has no base, so a guest address reads nothing

**2026-09-14** - orbistoun-gpu-vulkan, after worklog 560

The console's translated primitive shader does not draw because **every memory access in it is
outside the guest-memory window**, and an access outside the window reads zero and writes
nothing. That is the window behaving exactly as designed. What it lacks is a base.

## 1. How it was found, and one probe that lied

Worklog 560 established that a minimal translated primitive shader draws and grew it towards
the console's, feature by feature - the mask narrowing, a memory store, one parameter export,
two, the carry-producing address arithmetic. All of them draw. This unit did the reverse, cutting
regions out of the console's own instruction stream, and none of those drew either.

Between the two came a probe that was wrong: several variants supplied the positions as
literals instead of fetching them, which gives every lane the *same* position - three identical
vertices, a degenerate triangle, and no picture regardless of anything else. Those results said
nothing about the shader and were discarded. The check that caught it was running the known-good
shape with literal positions and watching it fail.

## 2. The answer

`MEMORY_WORDS` is sixty-four, and `address_within_window` asks whether `address >> 2` is below
it - an absolute range from zero. The GL cube's vertex buffer sat at `0x200900000` and its
canary at `0x200920000`. Neither is in the first sixty-four words of anything.

So the translated shader runs, fetches zeros, exports three identical vertices, and its canary
store lands nowhere. Every observation from worklogs 559 and 560 follows from that one fact,
including the one that looked most like "the shader never ran": the window still holding its
seeded bytes.

The predicate is right and this project already said so. `MEMORY_WORDS` carries the note:
*when real submissions arrive this becomes a base and a length, and an address outside them has
to be refused rather than wrapped - a store that silently lands somewhere else is the worst
failure this layer can produce*. Real submissions have arrived.

## 3. What this leaves

A test that draws a triangle from a translated primitive shader, with five variants that each
add one thing the console's shader does, all passing on a device. It is the smallest exercise of
the whole mesh path and it is worth keeping whatever happens to the window.

And a statement of the gap, which is a subsystem rather than a bug: the guest-memory window is
a *window*, and a guest's addresses are wherever the guest put them. Until it has a base, no
translated shader that touches memory can do anything, which is most of them.

## 4. Files

- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the variants, the finding in the
  doc comment, and the bisection scaffolding removed now that it has answered.

## Next

1. **Give the window a base.** The memory access path masks an address into a fixed window; it
   needs to subtract a base first and refuse what falls outside. That is `MEMORY_WORDS`'s own
   note, and it is what stands between a translated console shader and a picture.
2. The image subsystem, for record B's textured shader.
3. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
