# D991 - A device-addressed dispatch is traced on the host with its images

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

`orbistoun-cli trace-dispatch` runs a device-addressed capture (D980) on the host evaluator and
compares what it stores with what the device stores, over the same inputs. The host trace reads the
capture's guest ranges where they lay, and its images through `wave_trace_with_images`. A device
run places the guest ranges again and compares them as `guest-<n>.bin`. Stored images are compared
texel by texel. A capture that noted its source supplies its own program and context, so `--kept`
is no longer needed.

**What the host trace models.** Reads and writes:

- `image_bvh_intersect_ray` and its 64-bit form, through the node test the translator emits
  (`model::bvh`), in host arithmetic;
- `image_load`, `image_load_mip`, `image_sample_lz` and `image_sample_l` at level zero, of the
  captured image a descriptor names. The image is matched by its extent, its layers and its format;
- `image_store` and `image_store_mip`, recorded as texels (`WaveTrace::texels`);
- `buffer_load_format_x` through a descriptor of 32-bit elements, read as a word.

It also computes:

- `src_scc` as a source;
- inline `1/(2*pi)`;
- an output clamp, under the shader's `DX10_CLAMP` mode (`LaneProgram::clamping`);
- the `GLSL.std.450` operations, the transcendentals among them, in a trace only. A run that forms
  addresses still leaves them unknown.

**Several groups, or all.** `--group x,y` repeats. `--every-group` runs every group and compares
each stored image whole. A texel the device changed that no host store wrote also counts as a
difference.

**A global access's offset is signed.** The `OFFSET` field of a global or scratch access is a
signed twelve-bit number (RDNA 2 instruction set, `FLAT`). The reference assembler encodes
`offset:-2048` as `0x800`. The decoder hands the field as it lies. The translator, the host
evaluator and native lane runs read it through `model::flat_offset`, which sign-extends it for
those two segments. Before this, a probe's store at `offset:-2048` landed 2048 bytes above its
address on the device. The test is `a_global_access_below_its_address_reaches_below_it`.

**Measured on GTA V's ray-traversal pass at `0x74005b7e8600`.** Its 2,171 instructions run as
`[240, 135, 1]` groups of one wave of sixty-four, device-addressed (D964), two lanes an invocation
(D973). The capture is from run 347's in-world frames. The pass writes a one-channel shadow mask
at half resolution. It casts one shadow ray a pixel, wave-coherently: the wave walks the hierarchy
together on a stack held in lanes, and box and triangle nodes go to `image_bvh_intersect_ray`.

- The pass is deterministic on the device: four replays left the same output hash.
- With `--every-group`, all 32,400 groups ran to their end on the host, in 12 minutes. Traversal is
  deepest over the characters, up to 31,790 instructions a wave. All 152,045 texels the host stored
  are the device's, bit for bit, and the device changed no texel the host did not store. The mask
  is the same whole.
- The first comparison, over 360 groups, disagreed on 22 texels of 8,932. A probe stored registers to guest memory
  from patched code (`--patch`, `--jump`). It found the host reading a normal map's channels
  reordered twice. The capture already holds an `8_8_8_8_UNORM` image's texels in the order its
  selects give (`channel_order`, as the binding hands them to the device), and the host applied
  the selects again. With that fixed, nothing differs.
- The hierarchy's node format, read from the capture: the TLAS and the BLASes use only boxes of
  floats, apart from five type-6 nodes, which the guest sends to the instruction too. Leaf lists
  name triangle nodes. Almost all (82,185) are type 0, a single triangle with its word 15 (`9`)
  naming the barycentric order the test already returns. A node whose type selects another of
  several triangles, or other id words, is not exercised by this pass. Amended by D992, which
  completes the node tests for the rest. D993 makes the fused multiply-adds round once on the
  device and in the host trace alike.

So the pass's output is correct as translated. The coloured static on in-world frames is not this
pass's: it writes one channel, and its replayed mask is plain lit or shadowed, noisy only on the
characters, as a one-ray-a-pixel shadow is before its denoiser.

**Limits.** The device and the host approximate the transcendentals and `v_rcp_f32` differently in
their last places, as the guest's hardware does too. For this pass, one-ulp changes to each of them
on the host changed no texel. A sampled image is filtered at the eight sub-texel bits a device
uses, so a bilinear sample can differ in its last bits. Atomics are not modelled. The tile flags
the pass ORs into its buffer are compared only through the stored image.

**Rejected:**
- Comparing only the replay's output hash between builds: two builds agreeing says nothing about
  either being right.
- Matching a descriptor to a captured image by the order the program loads descriptors: the
  binding's order is the translation's, which the host does not run.
