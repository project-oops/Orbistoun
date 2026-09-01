# D059 - Relocations apply; "interception is linking" is now machine code

**decided** · 2026-08-19

D005 stops being a design statement here. Writing a host address into a PLT slot **is**
the interception - there is no hooking pass, and the guest calls whatever the slot
contains.

**Verified on real material.** The commercial executable reaches `Linked` with
**174,172 of 174,172 relocations applied** - zero unresolved, zero deferred, zero
unsupported. A bundled module applies 183 of 185, the two remaining being TLS.

Standard `Elf64_Rela` throughout; the vendor adds nothing. Two tables, and the split
matters: `DT_RELA` carries data relocations (172,790 in the executable), `DT_JMPREL`
carries the procedure linkage table (1,382) - one slot per imported function, and the
place imports become calls.

**Four decisions inside it:**

- **Unapplied entries are counted, never skipped silently.** `RelocationTally`
  distinguishes TLS-deferred, unsupported, and unresolved. An unrelocated pointer looks
  valid and is not, so a guest that limps past one fails somewhere unrelated and much
  later. The tally is what separates "this image is not ready" from "this image loaded
  and then behaved strangely".
- **Every write is bounds-checked against the image span.** A relocation pointing
  outside it is a corrupt or hostile table, and honouring it would scribble on
  unrelated memory. Refused rather than clamped.
- **Unresolved symbols are not written as zero.** A zeroed slot looks like a valid null
  pointer and faults somewhere unrelated. Until per-import thunks exist, every import
  resolves to a recognisable sentinel (`0x0000_DEAD_0000_0000`), so a crash address
  says immediately what happened.
- **The arithmetic is separated from the writing.** `value_for` computes what a
  relocation should write and is fully testable with nothing mapped - the D016 pattern,
  and it caught the signed-addend case (a negative addend read as unsigned lands near
  the top of the address space).

**Remaining for execution:** thread-local storage, then the entry jump. The executable
needs no TLS relocations at all, so the entry jump may be the only thing between here
and guest code running.

