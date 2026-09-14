# 560. A minimal translated primitive shader draws, so the fault is the console's own

**2026-09-14** - orbistoun-gpu-vulkan, after worklog 559

Worklog 559 established that the console's translated primitive shader does not execute: the
attachment keeps its clear and the guest-memory window still holds the seeded bytes at the word
the shader's own canary would have overwritten. This is the bisection it called for.

**A minimal translated primitive shader draws.** Twelve guest instructions - declare the
counts, fetch a vertex per lane, export the primitive, export the position - translated at the
mesh stage into 52,318 words, and every pixel comes back the colour the fragment shader wrote.

So the stage translation works end to end, and what is wrong is specific to the console's
shader rather than to the idea.

## 1. Four differences, each tested and each innocent

The minimal shader was grown towards the console's one feature at a time, and all four work:

| added | result |
|---|---|
| execution-mask narrowing (`exec_lo = 7`, the way a primitive shader says three lanes are vertices) | draws |
| a guest-memory store, through the `off` form the console uses | draws, **and the word lands** |
| a parameter export, the output a mesh module declares that the minimal shader has none of | draws |
| all of the above | draws |

The store one is worth stating twice: the console's canary never reached memory, and a store in
a minimal module reaches it immediately. Storing from the mesh stage is not the problem.

## 2. What is left, and how to finish it

Every instruction word in the minimal shader is the console's own where it could be - its packed
primitive word, its export words, its no-base store - so the differences that remain are about
what surrounds them:

- **Order.** The console exports its primitive first, under a mask of one lane, and fetches and
  exports its vertices afterwards. The minimal one fetches first.
- **Address arithmetic.** The console forms a 64-bit address from two scalar literals with a
  carry-producing add; the minimal one uses the register directly.
- **Saving the mask.** The console keeps the entry mask in a scalar register and restores it at
  the end.
- **Size.** 171,095 words against 52,318.

The next step is mechanical and is the reverse of this one: take the console's own instruction
stream and *delete* a region at a time - the canary blocks, the mask save and restore, the two
parameter exports - until it draws. The word list is already in the test, so each variant is a
slice.

## 3. What this unit leaves behind

`a_minimal_translated_primitive_shader_draws` is a regression test as well as a probe: it is the
smallest thing that exercises the whole mesh path - declaration, fetch, primitive, position, and
now a store and a parameter - and it asserts nothing yet because what it measures is still the
open question. It will assert once the console's shader draws, because then the question is
which of the two changed.

## 4. Files

- `crates/orbistoun-gpu-vulkan/tests/console_fragment.rs` - the minimal shader and the four
  variants of it.

## Next

1. Finish the bisection by deletion, as above.
2. The image subsystem, for record B's textured shader.
3. `REQ-20260914T1720Z-9c4a`, which settles the payload split D688 assumes.
