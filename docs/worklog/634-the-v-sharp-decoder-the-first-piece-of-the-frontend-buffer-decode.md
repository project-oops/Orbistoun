# 634. The V# decoder: the first piece of the frontend's guest-buffer decode

**2026-09-16** - a concrete buffer-descriptor decoder in the frontend, so a guest's V# read out of
its scalar registers becomes a base, size and stride a host can bind; and what the rest of the decode
still needs

## Where this fits

Worklog 633 gave the backend a `Buffer` arm and a dispatch that runs against a resident buffer - but
nothing on the **frontend** produces a `Buffer` resource from a real guest. This is the first piece of
that frontend decode: turning a guest's buffer descriptor into the numbers a host needs.

## The V#, concretely

A buffer resource - a "V#" - is four dwords. `orbistoun-translate`'s `read_buffer_resource` already
decodes it, but as *arithmetic*: the descriptor lives in registers the shader loads at run time, so
every field is a value emitted as a SPIR-V expression rather than a number. The frontend has the
**concrete** values - the four dwords a guest wrote into its scalar registers - so it can decode them
to numbers. `decode_buffer_descriptor([u32; 4]) -> BufferDescriptor` does exactly the same bit layout
as `read_buffer_resource`, evaluated:

- base address in bits 47:0 (dword 0, and the low half of dword 1);
- stride in bits 61:48, masked to fourteen bits;
- record count in dword 2 - a byte length for a raw buffer (stride 0), a count of `stride`-byte
  records otherwise, which `byte_len()` resolves;
- `unsupported` when the descriptor asks for swizzled addressing (bit 63) or add-thread-id (bit 119),
  which this does not model - a host cannot honour those by binding a plain range, so it is a refusal,
  not a buffer, exactly as the translator refuses them.

`buffer_descriptor_at(&[RegisterWrite], first)` reads the descriptor a guest left in four consecutive
registers from `register_writes`, taking the most recent write to each (the live value the shader
would see) and refusing when any of the four was never set.

The decode is the same reference layout the translator uses, kept in step by decoding to the same
fields; it is not a second source of truth for the format.

## What the rest of the frontend decode still needs

Producing a `Buffer` resource for a real guest dispatch needs two things this does not have yet, and
both are their own work:

1. **Which registers hold each buffer's V#.** A compute shader loads its descriptors from user-data
   scalar registers, and *which* four registers is decided by the shader - it is `read_buffer_resource`'s
   `first`. The translator knows it internally but does not export it: `Translated` carries the SPIR-V
   module and nothing about which descriptors it read or which bindings they map to. So the next piece
   is **descriptor reflection out of the translator** - a list of `(first register, binding, kind)` per
   access - without which the frontend cannot know where to look.
2. **Reading the buffer's bytes.** Once the base and length are known, the buffer's contents are read
   from guest memory (the pipeline already holds `GuestMemory` for shaders) and handed to the backend
   as a `Buffer` resource, with a `BindBuffer` naming the SPIR-V binding the reflection gives.

This piece is the decoder those two will use; it is built and tested first because it is pure and has
a measured layout, where the other two are plumbing and a translator change.

## Made to fail

- `a_buffer_descriptor_decodes_its_base_stride_and_records` - a descriptor built to a known base,
  stride and count decodes to exactly those, and `byte_len` gives `records * stride`.
- `a_raw_buffer_is_bytes_and_swizzling_is_unsupported` - stride 0 makes `records` a byte length, and
  swizzle-enable / add-thread-id each mark the descriptor unsupported.
- `a_descriptor_reads_the_live_register_values` - the latest write to each register wins, and a
  descriptor missing one of its four registers is `None`.

## Gate state

`cargo test -p orbistoun-gpu` 42 pass in the lib (3 new); `cargo clippy -p orbistoun-gpu
--all-targets` clean (the `similar_names` the first draft tripped is gone - the dwords have their
field names now); fmt clean. Full-workspace gates running.
