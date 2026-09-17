# 572. The plain sampling form needed nothing new, and the census said so

**2026-09-15** - orbistoun-translate and orbistoun-gpu-vulkan, after worklog 571

Both outstanding findings wait on hardware measurement, so the question was what else there is.
The roadmap has a standing answer to that question, written after being wrong about it three
times:

> **before recording this section as empty, run the generators and read what they refuse.**
> They have refused something every time.

So it was run. `orbistoun-cli shaders` over the shader fixture corpus:

```
shaders      10 of 14 translate
instructions 177 of 184 translatable

shaders     uses  known  instruction
      2        3   yes  image_sample  (MIMG:0x20)
      1        2   yes  image_load  (MIMG:0x0)
      1        1   yes  image_store  (MIMG:0x8)
      1        1   yes  image_sample_l  (MIMG:0x24)
```

`image_sample` blocks two shaders, more than anything else, and its solved operand layout is
**character for character the same** as the one already translated. The whole of the work was
choosing which SPIR-V instruction to emit.

## 1. Why it was already done and nobody had noticed

Worklog 566 established that a guest's `_lz` and the plain form are two different SPIR-V
instructions rather than one with an option, and built an oracle that emits either. Worklog 568
translated the `_lz` form and D690 settled the descriptor question for *any* sample. So by the
time this was looked at, the plain form needed: one name in the supported list, one arm in the
dispatch, and a branch choosing the implicit instruction over the explicit one.

The stage guard already covered it and covers it better than expected. A sample refuses at any
stage but the fragment one, for two reasons that happen to agree - the harness binds its image
with fragment stage flags, and the vector type a sample answers with is declared by the colour
output. The implicit form adds a third: it picks its level from the **derivatives of the
coordinate**, which only a fragment stage has. Three independent reasons, one guard.

## 2. After

```
shaders      11 of 14 translate
instructions 180 of 184 translatable
  FURTHER  11 of 14 shaders complete (+1), 180 of 184 instructions (+3)
  cleared image_sample
```

The device test runs both forms through the same four-quadrant assertions and compares each
against the hand-written oracle for **its own** level form, byte for byte. Checking a translation
of the implicit instruction against an oracle emitting the explicit one would have passed on this
texture - it has one level - and proved nothing.

## 3. What is left, and why each one is not free

Three instructions, all MIMG, and none is the same shape as the two that are done:

- **`image_load` and `image_store`** read and write an image with no sampler. That is a storage
  image on the host, a different descriptor type and a different binding from the combined image
  sampler the subsystem has. It is a host-side piece of work before it is a translation.
- **`image_sample_l`** takes its level from an address register rather than naming zero. Which
  register is not in the encoding and not in the solved layout: the layout names the first
  address register, and the level's position among those that follow depends on the
  dimensionality, which is in the descriptor. Implementing it means claiming an operand order
  nobody here has measured, and this project refuses to do that from a guess (D096's whole
  reason). It waits for a probe.

The roadmap's G7 row is updated with the measured numbers. It had said 120/127 and named
`image_sample` as needing the resource model, which had been true and stopped being true.

## 4. Files

- `crates/orbistoun-translate/src/model.rs` - `image_sample` takes the mnemonic and picks its
  instruction; `image_sample` in `SUPPORTED` and in the dispatch.
- `crates/orbistoun-gpu-vulkan/tests/translated_sampling.rs` - both opcodes, each against its
  own oracle.
- `docs/roadmap/015-phase-6-s-contents-built-ahead-of-it.md` - G7, measured.

## Next

1. `REQ-20260914T2348Z-4e71` - which register carries a buffer address, for a derived window.
2. `REQ-20260914T1720Z-9c4a` - the payload split D688 assumes.
3. A probe for the address-register order a levelled sample uses, if one is wanted before the
   storage-image work.
