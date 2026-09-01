# D152 - The entry point was reading a stray host pointer, and it looked like progress


**decided** · 2026-08-20

Found by accident, which is the only reason it was found at all.

Guest threads need an argument passed to their body (`void *start(void *arg)`), so
`enter_guest` grew a general form that puts a value in `rdi`. The plain form delegated to
it with zero - reasoning that zero is what an entry point taking no argument would find
there anyway.

Two unrelated titles immediately went from reaching thousands of bytes into their own
code to faulting with **`read of 0x0` at the identical offset, `image+0x7a`**. The
identical offset across two titles is what gave it away: that is not title code, that is
the entry path.

The previous behaviour was never "no argument". `rdi` was declared as a clobber, so it
held **whatever the compiler happened to leave there** - a stray host pointer. The guest
entry point dereferenced it, got plausible-looking garbage, and carried on for another
sixty thousand bytes and thirty-seven distinct imports.

That is the exact failure principle 3 exists to prevent, and it had been reading as
progress for days. Every measurement taken through that path was taken on undefined
behaviour.

**The finding itself:** a process entry point on this platform dereferences its first
argument register immediately. `enter.rs` already documented that a real entry expects a
process argument block - count, argument and environment pointers, an auxiliary vector -
and recorded it as deliberately not built. What it did not know is that the entry point
does not tolerate its absence for a single instruction.

So there is now a real one: a zeroed page this crate owns, never written, never freed.
Same reasoning as a thread handle's control block (D151) - the layout is not known from
any lawful source, so every field reads as zero. A guest reading a count gets none; a
guest reading a pointer gets null and can check it. Nothing is invented.

With it in place both titles are back to where they were: thirty-seven imports,
`image+0xf2f6`, verdict `same`. **That is parity, not progress** - the number is the same
and now it means something.

Two things worth keeping from this:

- **An undefined register is not an empty one.** Anywhere a clobber stands in for a value
  the guest might read, it is a stray host value, and the guest cannot tell.
- **Changing something inert is a test.** Nobody would have written an experiment to
  check whether the entry point reads `rdi`. Setting it to a defined value and watching
  two titles break identically answered it in one run.

