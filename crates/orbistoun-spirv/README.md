# orbistoun-spirv

Building SPIR-V modules.

It holds the binary word format: header, instruction encoding, identifier allocation
(`Builder`), and the type and constant deduplication a valid module needs.
[orbistoun-translate](../orbistoun-translate/) builds on it.

## Rules

- **No guest knowledge.** This crate has never heard of wavefronts, execution masks or vector
  registers. It holds the same boundary against the guest that `orbistoun-gpu` holds against
  Vulkan; translation lives above it and maps guest semantics onto what is here.
- **Words, not text.** SPIR-V is a binary format of 32-bit words, and nothing here goes via an
  assembler: a translator that emits text and shells out to `spirv-as` cannot run where it is
  needed.
- **Identifiers are handed out, never chosen.** The header declares a bound that must exceed
  every result id. `Builder` allocates the ids, so the bound and the ids in use cannot
  disagree - a mismatch that makes a module invalid for a reason nobody can see by reading it.
