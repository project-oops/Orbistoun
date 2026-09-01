# The retarget, and the tree is red on purpose


`MCPU` flipped from `gfx900` to `gfx1030`. **The tree does not currently pass**, and the
state is written down here rather than left to be rediscovered.

Landed and working:

- **The target is one constant.** `orbistoun-gen`'s target module; it was four, in two generators, a
  probe script and the fixture headers. Part of why the wrong generation survived months.
- **A documented toolchain.** `tools/toolchain/setup.sh` builds a VM with the reference
  assembler; `run.sh` runs a generator inside it. REFERENCES.md said the tables were
  derived by experiment, and until now the machine that could redo the experiment was
  undocumented - readable by anyone, reproducible by nobody.
- **A guard that makes a half-retarget impossible.** Every table declares the generation
  it describes, and `EncodingTable::builtin` refuses a set that disagrees. The
  hand-written file states intent; the generated ones record what a tool actually ran
  against.
- **The operand table is regenerated** for the new target, 48 opcodes solved.
- **`orbistoun-gen encodings`**, new: solves each family's mask, value, opcode position
  and width from assembled bytes, so those rows stop being transcribed.

### Surprises

- **67 of 69 first-run rejections were a wavefront width, not the ISA.** The reference
  defaults this generation to 32-lane waves, where `vcc` and `s[4:5]` are the wrong
  spelling. Two rejections were real: `v_mad_f32` is gone and `v_add_u32` is spelled
  `v_add_nc_u32`. See D141 - and note the encodings are identical either way, which is
  what makes the width a translator concern rather than a table one.
- **This generation's VOP3 sits exactly where the previous one's VINTRP did.** So every
  long-form vector instruction decoded as an interpolation, and the generator reported it
  as `VINTRP:0x0 (v_mul_f32_e64)` - a family and an opcode that disagree about what the
  instruction is. Nothing else would have caught this: an opcode number for the wrong
  generation still lands on a *real* instruction.
- **The generator died on the first rejected probe.** It now collects every one and
  prints the list, because a retarget needs the whole worklist rather than a game of
  deleting lines until it runs.
- **A family's opcode field is a span, not a set.** The first solver demanded that the
  bits differing between mnemonics be contiguous; four mnemonics with opcodes 0x01, 0x02,
  0x03 and 0x41 differ in bits 0, 1 and 6, so it refused five perfectly ordinary
  families. Filling the span fixed all five at once.
- **Masks cannot be solved from within a family.** Bits constant across the probes are
  indistinguishable from bits that identify the family, so the mask came out too wide -
  which silently drops every member whose opcode uses the over-claimed bits. Asking what
  *separates* the families instead makes it independent of which opcodes anyone probed.

### Outstanding, in order

1. `encodings.toml` still holds the previous generation's family rows. This is why the
   tree is red, and the failure is loud and accurate: the target guard names it.
2. The family solver gets 12 of 13. `SOPK` does not solve; `SOP1` solves visibly wrong
   (a one-bit opcode); `SMEM`, `MUBUF` and `VOP3` come out with masks narrower than the
   format really is, because no probe file declares the neighbouring formats they need to
   be separated from. All three are probe-coverage problems, not solver problems.
3. `mnemonics.toml` and the fixtures need regenerating - `orbistoun-gen fixtures` needs
   `llc`, which the VM has, and it has not been run yet.
4. `SUPPORTED` still names `v_mad_f32` and `v_add_u32_e32`, which this generation does
   not have. The retarget guard is what will say so.

