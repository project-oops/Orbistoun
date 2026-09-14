# 549. The image family named and solved, and a mask that solved one bit too wide

**2026-09-14** - orbistoun-gen and orbistoun-shader, after worklog 548

Worklog 548 left `image_sample_lz` as the one thing between the GL cube's textured pixel
shader and a translation: decodable, unnamed, and in a family with **no operand layout for
any opcode**. Both are fixed. The textured shader now fails at the same instruction as the
untextured one - a flat store at `0x30` - rather than at its sampling instruction at `0x84`,
and the corpus worklist has nothing left in its ordinary section: 196 of 200 instructions
translatable, and the only two entries are the pair waiting on subsystems.

## 1. Five opcodes named, five layouts solved

`image.ll` compiles to exactly one MIMG instruction, `image_sample` with `dmask:0x1`, because
that is what the single intrinsic anybody can write from IR produces. A hand-written
fixture, `tools/shader-fixtures/sampling.s`, reaches the rest.

| opcode | name | operands |
|---|---|---|
| 0x00 | `image_load` | data, address, resource, mask |
| 0x08 | `image_store` | data, address, resource, mask |
| 0x20 | `image_sample` | data, address, resource, sampler, mask |
| 0x24 | `image_sample_l` | data, address, resource, sampler, mask |
| 0x27 | `image_sample_lz` | data, address, resource, sampler, mask |

The resource is **eight** consecutive scalar registers and the sampler **four**, and each is
named by a five-bit field holding a quarter of its base register - the same quarter-scale
reading the buffer descriptors already use, which is why the solver could find them at all.
The probes put resources at `s[80:87]` and `s[92:99]`, past what a four-bit field reaches, so
the width is pinned by samples rather than by assumption.

`image_sample` therefore leaves `NO_OPERANDS_DECODED`, the inventory of instructions the
decoder knowingly reports nothing for. It was listed there for the reason MTBUF once was:
that a descriptor field read without modelling what it points at produces a number that looks
like a register. The field turns out to name where the group starts, which is a decode
question and is now answered; what the descriptor points at is the translator's problem.

## 2. Two token rules the solver was missing

- **`dim:SQ_RSRC_IMG_2D` is a symbolic modifier**, like MTBUF's `format:[BUF_FMT_32_UINT]`,
  and the pattern only matched the bracketed spelling. Unrecognised, it became an operand
  with no reading any field could explain, and every image opcode reported as unsolvable -
  which reads as a gap in the probes and was a gap in the token rules. The widened pattern
  deliberately refuses a value beginning with a digit, so `offset:16` stays an operand: an
  offset is a field a translator must read, and dropping one silently puts an access
  somewhere the guest did not mean. Both directions have a test.
- **The differential test had to learn the same split.** It compares decoded operands against
  the reference's printed text, and the reference prints `dmask:0xf` where the decoder
  reports `0xf`. Without splitting the name off, a correctly decoded mask has nothing to
  match and every image instruction fails there the moment its layout is solved.

## 3. The mask solved one bit too wide, and the refusal is what caught it

`dmask` is four bits with `unorm` immediately above it. The first solve wrote **five**, and
it fitted every sample perfectly - because no probe set `unorm`, so the fifth bit was zero
throughout. A shader sampling with unnormalised coordinates would have decoded its mask as
sixteen greater, silently.

Measured rather than argued: assembling one instruction with and without `unorm` moves bit
12, and `glc` moves bit 13. Probes that set `unorm` then left the mask with **no candidate at
all** and the three sampling opcodes reported unsolvable - the solver refusing rather than
approximating, which is the design working. The answer was missing from the search: four was
not among the immediate widths it tries. Adding it produced the narrow field.

Two small consequences worth knowing. `unorm` joins the modifier lists on both sides - it is
skipped as an operand and still worth *setting* in a probe, which is a distinction the
comment now makes, because every modifier in that list is a bit somewhere and a neighbouring
field can be pinned by moving it. And two typed-buffer opcodes moved from a four-bit to a
five-bit reading under the wider search, adopted the same way the three existing widenings
were.

## 4. What the oracle says now

The textured pixel shader's failure moved from `0x84` - "has no operand layout; cannot
translate what it operates on" - to `0x30`, the same flat store that stops the untextured
one. Both records, both shaders, one remaining question between them: whether the scalar-base
code `0x7d` means "no base" (`REQ-20260914T1402Z-b6d8`, still open on the obSCEne bus).

The worklist's ranked section is empty. Its "waiting on a subsystem" section holds the two:

- `s_sendmsg`, waiting on what a primitive shader is to this translator (548 §4).
- `image_sample_lz`, blocked here with its reason: its resource operand names eight scalar
  registers holding an image descriptor and its sampler operand four more, both of which the
  decoder now reads. What they point at is a descriptor this translator has no model for and
  a host image view nothing declares - the SPIR-V emitter has no image type, no sampled-image
  type and no sampling instruction. A subsystem, not an encoding gap.

That is the honest end of the naming work for this corpus: everything the GL cube runs is now
named and its operands are read, and the three things still refusing are a measurement, a
design decision and a subsystem.

## 5. Provenance

Names from `llvm-mc` and `llvm-objdump` in the toolchain VM; operand layouts solved from
assembled probes; the `unorm` bit position measured by assembling the same instruction twice.
Nothing disassembled from a vendor module, nothing taken from the repositories audited in
worklog 541.

## Files

- `tools/shader-fixtures/sampling.s`, `tools/shader-fixtures/probes/image.s` - new.
- `crates/orbistoun-gen/src/patterns.rs` - the symbolic-modifier pattern;
  `crates/orbistoun-gen/src/operands.rs` - `unorm` in the modifier list, four in the immediate
  widths, and two tests for the token rule in both directions.
- `crates/orbistoun-shader/data/{mnemonics,opcode-operands}.toml`,
  `crates/orbistoun-shader/tests/fixtures/sampling.{gcn,txt}`,
  `crates/orbistoun-gen/tests/fixtures/transcripts/operands-image.*` - generated.
- `crates/orbistoun-shader/tests/differential.rs` - the fixture row, the named-immediate
  split, `unorm`, and `image_sample` leaving the silent-decode inventory.
- `crates/orbistoun-translate/src/model.rs` - `image_sample_lz` blocked, with why.

## Next

1. `REQ-20260914T1402Z-b6d8`: the flat base. Both fragment shaders now stop there, so it is
   the single highest-payoff answer outstanding.
2. What a primitive shader is to this translator - `s_sendmsg` and `exp prim` both wait on it.
3. An image subsystem, if the textured path is to translate rather than decode.
