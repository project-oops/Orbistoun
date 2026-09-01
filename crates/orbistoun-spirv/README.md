# orbistoun-spirv

Building SPIR-V modules.

**Models:** the binary word format - header, instruction encoding, identifier allocation,
and the type and constant deduplication a valid module needs.

**Deliberately fakes:** nothing.

**Design note.** **This crate knows nothing about the guest.** It has never heard of
wavefronts, execution masks or vector registers, and it must stay that way - the same
boundary `orbistoun-gpu` holds against Vulkan, for the same reason. Translation lives above
this and maps guest semantics onto what is here.

**Words, not text.** SPIR-V is a binary format of 32-bit words. Nothing here goes via an
assembler, because a translator that emits text and shells out to `spirv-as` cannot run
where it is needed.

**Identifiers are handed out, never chosen.** The header declares a bound that must exceed
every result id. `Builder` allocates them, so a mismatch between the bound and the ids in
use cannot happen - which is a whole class of module that validates as malformed for a
reason nobody can see by reading it.

**Status:** complete for what the translator emits.
