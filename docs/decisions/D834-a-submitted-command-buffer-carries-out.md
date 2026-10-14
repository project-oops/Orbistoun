# D834 - A submitted command buffer carries out its write-address commands

**Status:** decided
**Date:** 2026-10-14
**known_by:** assumed (the write's width; the command's encoding is measured)

`sceKernelAprSubmitCommandBufferAndGetResult` carries out the reads queued on the command buffer,
then walks the buffer the object has bound - its first `used` bytes, the object's +4 - and carries
out each write-address command in order: the value, zero-extended, is written as the eight-byte
word at the command's address. A command that is not a write-address stops the walk; it and
everything after it are reported and not carried out.

The command format - the opcode, the flags byte, the address's high bits in the header, the low
word, the value shifted down two - is the one `sceAmprCommandBufferWriteAddress_04_00` appends, as
a console appended it (REQ-aw01, REQ-aw02). It lives in the kernel's `apr` module, which both
appends and walks it, so the two never drift.

**Why:** a write-address after a read is how a title learns the read is done. Without it the word
it polls keeps the value it was primed with, and the completion it waits for never comes. Grand
Theft Auto V's main thread waited on exactly that: the wait sub-loop (D832) showed the word it read
never written. With the commands carried out, its main thread moved past its startup wait to a new
wall.

**Assumed:** the write is eight bytes, the value zero-extended. The command carries 32 bits of
value, and a write-address to an address not eight-aligned is refused (REQ-aw01), which fits an
eight-byte word. REQ-20261014T0130Z-rf03 asks for the slot's eight bytes, primed `cc`, after a
submitted write and its wait; the measurement settles the width.

**Rejected:**
- Writing four bytes: a slot primed with all ones would keep its high half, and a title comparing
  the word would never see its value.
- Carrying out only the commands appended since the last submit: what a submit consumes is not
  measured; the title resets its buffers itself.
