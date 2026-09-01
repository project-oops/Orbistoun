# D101 - Guest memory is a second binding, never a second half of the first


**Status:** assumed

A translated module now binds two storage buffers: the observation window registers are
copied into at binding zero, and guest memory at binding one. One buffer holding both,
with memory starting at a known offset, would be fewer descriptors and less code.

Rejected because a guest address would then be able to reach the observation window. A
store landing there rewrites the registers a test is about to assert on, so the failure
presents as a register bug in an unrelated instruction. The addresses in the tests are
chosen and would not do that today; the addresses in the first real shader are not
chosen by anyone here.

Guest memory is currently sixty-four words indexed directly by the address, which is a
placeholder and marked as one. A real mapping is a base and a length, and an address
outside them has to be refused rather than wrapped - a store that silently lands
somewhere else is the worst failure this layer can produce.

### Confirmed in part, and the wrapping fixed

**The binding split is decided.** Nothing has challenged the reasoning and today's buffer
work leaned on it: an out-of-range guest store must not be able to reach the observation
window, or a memory fault presents as a register fault.

**The direct indexing is still a placeholder, but it no longer aliases.** `word_index`
masks the index to keep it legal - reading a storage buffer out of range is undefined
behaviour, so that part has to happen - and masking is not clamping. A store one word past
the end landed on word *zero*, and every symptom looked right: the shader ran, memory
changed, and the change was somewhere the guest never asked for. A guest overrunning a
buffer is an ordinary bug; a translator that turns the overrun into a plausible-looking
corruption of the start of memory makes that bug unrecognisable.

Accesses now ask `address_within_window` and answer the way the hardware answers an
out-of-range buffer access (D147): reads give zero, writes do not land. Multi-word
accesses step by *address* rather than by index, so an access that begins inside the
window and ends outside it is caught per word rather than wrapping its tail.

The window size is carried on the model rather than read from a constant, so a test can
widen it to reach an address the default cannot hold.

### What the real mapping is waiting on

Not a design - a fact. A shader's address is a **GPU** virtual address, and whether that
equals the guest address is a vendor OS policy, not an ISA property, so the instruction-set
reference cannot answer it and neither can FreeBSD.

It is **measurable rather than researchable**, and the strongest route needs no capture:

1. **Match addresses against allocations.** Every guest allocation goes through our own
   shim, so the address handed out is known. When a submission contains an address, check
   whether it falls inside a region we allocated. Identity either holds or the offset is
   visible. This is the shape `Pipeline::submit` already uses to reconcile the two routes
   to a shader address, and `GuestMemory::read` gives a weak form of it for free.
2. **The graphics library's own signatures.** A submit call taking a plain pointer is
   evidence the two spaces coincide.
3. **Observed failure.** A wrong mapping renders visibly wrong, which is what the
   framebuffer oracle exists to catch. The backstop, not the plan.

