# D810 - the cube-map face instructions follow the chain z then y then x

**Status:** decided
**Date:** 2026-10-12

`v_cubeid_f32`, `v_cubesc_f32`, `v_cubetc_f32` and `v_cubema_f32` take a direction's three
components and pick the face it leaves the cube through. The major axis is chosen in one chain:
z when its magnitude is at least x's and y's, otherwise y when its magnitude is at least x's,
otherwise x. A component counts as negative when it is less than zero, so `-0.0` is positive.

| major | id | sc | tc | ma |
|---|---|---|---|---|
| +x / -x | 0 / 1 | -z / z | -y | 2x |
| +y / -y | 2 / 3 | x | z / -z | 2y |
| +z / -z | 4 / 5 | x / -x | -y | 2z |

The face numbers, the coordinates and the doubled major component are NIR's `cube_amd`
(`src/compiler/nir/nir_opcodes.py`), which ACO lowers to exactly these four instructions over the
same three sources and packs as (tc, sc, ma, id) (`aco_select_nir_alu.cpp`). For every input with
no NaN the chain and NIR's tests agree, ties included: z wins over y and x, y over x.

`known_by = assumed` for a component that is NaN. NIR's evaluator then matches no axis and leaves
the results at zero; the chain falls through to x. Neither is a measurement of the instruction,
and the chain is the one that defines all four results, the face id included, which NIR leaves
unset.

**Why:** PPSA03416's pixel shader samples a cube map, and nothing translated the four
instructions; its one draw that uses them was refused.

**Rejected:**
- NIR's tests transcribed as written: they leave the face id undefined when no axis is major.
