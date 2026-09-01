# D151 - A thread handle is an address, because the guest dereferences it


**decided** · 2026-08-20

The first version made `ThreadHandle` a small opaque integer, with a comment explaining
that a handle is passed back rather than read through, so it never needs to be an address
- and that a small integer cannot be mistaken for a pointer if it leaks.

The evidence against that was already in the knowledge file, written a day earlier under
D125: an unimplemented `scePthreadSelf` returned the placeholder `0x7FFF0001`, and a title
faulted with **`read of 0x5`** - that error code being dereferenced at an offset. The
guest reads fields out of what this call returns.

A handle of `1` reproduces exactly the same fault at a lower address. The "opaque integer"
reasoning was sound in the abstract and contradicted by a measurement this project had
already made and written down.

So a handle is the address of a leaked, zeroed, eight-byte-aligned block. Three
consequences, each chosen rather than inherited:

- **Never written.** The real structure's layout is not known from any lawful source, so
  every field the guest reads is zero. For a pointer field that means null, and a guest
  that checks for null takes its own error path instead of dereferencing garbage.
  Inventing plausible field values is exactly what principle 3 forbids.
- **Never freed.** A handle kept past its thread's life then reads as zeroes rather than
  as a use-after-free. The count is bounded by how many threads a title makes.
- **Checked before use.** `is_issued` gates any guest-supplied handle, because now that a
  handle is an address, believing one the guest invented would turn a guest bug into a
  host memory access.

The same applies to lock handles, for the same reason.

The general lesson is the one worth keeping: **the knowledge file is not a report, it is
an input.** It contained the answer before the question was asked, and only got consulted
because a test failed for an unrelated reason.

