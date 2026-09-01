# 2026-08-19 - The worklist starts moving


**Done.** Closed the loop the whole project is built around, then walked one step down
it.

- `predicated::SUPPORTED` - the instructions the translator handles - is public, and
  `instruction` consults it before dispatching, so the list is authoritative rather
  than a second opinion.
- `orbistoun-cli shaders` asks the translator what it supports instead of assuming
  nothing.
- Three more instructions, each a TDD cycle: `s_waitcnt`, a scalar register file, and
  `s_mov_b32`.

**Verified.** Gate green, device tests executed. Seven execution tests pass on hardware.
The worklist moved from **0 to 45 of 106 instructions translatable** on four
instructions, and now ranks real work:

```
      5    6  global_store_dword  (FLAT:0x1c)
      4    5  s_load_dwordx4      (SMEM:0x2)
      3    3  v_add_f32_e32       (VOP2:0x1)
```

**Surprises.**

- **Four instructions covered 42% of the corpus.** `s_endpgm`, `s_waitcnt`,
  `s_mov_b32` and `v_mov_b32` are apparently what shaders are mostly made of. Worth
  remembering when the remaining count looks discouraging: instruction *kinds* are
  distributed nothing like instruction *counts*, which is the entire argument for
  ranking by shaders blocked rather than by frequency (D086), arriving from the other
  direction.

- **The agreement test caught an error in its own fixture.** `0x7E00_0080` was written
  meaning `v_mov_b32` and is opcode *zero* - the opcode bits sit at 9 and were left
  out. The test reported that the translator refused something the list claimed, which
  was true and for the opposite reason to the one suspected.

- **Two instructions that legitimately emit nothing tripped `match_same_arms`.** Merged
  into one arm with both reasons written out, rather than split to satisfy the lint -
  splitting would have implied the bodies differ.

**Not done.** Still no arithmetic, no memory access, no control flow. The next three
blockers are memory - `global_store_dword` and the scalar loads - which need guest
memory modelled as a second buffer, and float arithmetic, which needs the register file
bitcast rather than treated as `u32`.

And it remains **unmasked**: register writes ignore the execution mask, because nothing
diverges yet. That is the largest unbuilt part of D098 and it wants doing together with
control flow.

