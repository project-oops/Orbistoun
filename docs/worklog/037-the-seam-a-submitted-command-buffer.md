# The seam: a submitted command buffer reaches the GPU


A command stream now produces a running shader. Packets in, register writes out, a shader
address among them, the shader fetched from guest memory, decoded, translated, and
executed on a real device leaving the value it was supposed to leave. Nothing in that
path calls the translator directly - the shader is found because a guest asked for it,
which is the only way one is ever found in a real frame.

**This was the missing thing, and it had been missing the whole time.** Packet walking,
register decoding, shader translation and the backend command vocabulary all existed,
all had tests, and none of them touched. Everything built in this subsystem so far was
verified against shaders its own tests handed it. A translator nothing calls cannot be
wrong in a way a test would notice.

Closing it needed three things that were not obvious until the path existed:

- **A shader in guest memory has no length.** `decode` decodes the slice it is given,
  which is right for a fixture and wrong for an address. `decode_program` stops at the
  instruction that ends a program and says whether it found one. Without that, the
  rubbish after a shader desynchronises the decode and a perfectly good shader reports as
  untrustworthy - so the fixture deliberately puts rubbish after the shader, and that
  test would have passed for the wrong reason if the window had been sized exactly.

- **The cache is keyed on bytes, not addresses.** Address keying is right in the common
  case and wrong in three others, one of which - a guest writing a different shader to
  the same address - draws the frame with code that is no longer there and indicates
  nothing. Decode first, hash the shader's actual extent, then translate only on a miss:
  decoding is linear and translation expands every instruction across sixty-four lanes,
  so the expensive half is the one behind the cache.

- **A failure is reported with its address and reason, not skipped.** Most real shaders
  will contain an instruction that does not translate yet. That must arrive as a ranked
  worklist entry, not as a draw that quietly did nothing.

**Surprises.**

- **`opcode_for_register` was nearly `set_register_opcode`.** The first version answered
  "an opcode that writes registers", took the lowest-numbered one, and underflowed the
  offset for every register below its base. Several opcodes write registers, each to a
  class with its own base, and *which one reaches a register is decided by the register*.
  The overflow was caught because Rust checks subtraction in debug; the same mistake in a
  release build, or in a language that wraps, is a packet naming a register four billion
  away from the intended one.

- **The end-to-end test found nothing wrong.** Worth recording, because it is the
  unusual outcome in this subsystem and it is evidence about the pieces rather than about
  the test: each one had been verified in isolation carefully enough that joining them
  produced no surprises. The two driver faults earlier in this session were both in code
  that had *not* been verified against a real device, which is the pattern.

**What this does not do.** A translated shader still writes into an observation window
and reads a sixty-four-word placeholder for guest memory, so a real shader could be
found, translated and dispatched today and would compute into a debug buffer. The
resource model - descriptors, buffers, images, render targets - is what turns that into
a frame, and `exp` is refused for exactly that reason (D104). That is the next
structural piece, and unlike this one it cannot be validated without a real submission to
check the bindings against.

