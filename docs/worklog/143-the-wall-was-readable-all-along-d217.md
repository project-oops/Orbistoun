# 2026-08-24 - The wall was readable all along (D217)


**Done.** The dump path's readable window corrected, argument classification split three
ways, and both current walls narrowed to one class by measurement.

### An off-by-one-page, in the worst possible place

`orbistoun-worker` declared guest memory dereferenceable across
`(GUEST_STACK_BASE, DEFAULT_STACK_SIZE)` - the values it had handed to
`GuestStack::reserve` - instead of the span the stack reports. `reserve` puts a guard page
at the base, so the real span sits a page higher at both ends. The declared one therefore
**offered the page mapped to fault** and **refused the top page of real stack**.

`libkernel::0x6abac2f3dc6f8cee`, the lead on `image+0xafc959`, is called with exactly one
pointer, and it lands 712 bytes inside the excluded page. So the technique that identified
the other two unnamed libc functions had never been able to run against the thing on the
biggest wall - it reported the argument as a count, every time.

The fix is to ask the stack rather than re-derive its span. `lowest_usable()` and `len()`
already existed and were unused.

### What it said once it could speak

`arg1 = 0x100000`, and the fault carries `rdx = 0x100000`. **`0x100000 - 0x20 = 0xfffe0`,
the faulting address exactly.** The guest asks for a megabyte aligned to `arg3` (256 KB)
and then indexes `base + size - 0x20` from a base of zero.

Two candidate sources for that zero, both eliminated by measurement rather than reasoning:

- **not the stub return** - an unimplemented stub answers a non-zero placeholder;
- **not unwritten memory** - `ORBISTOUN_STACK_FILL=5a` fills the stack, `arg0`'s first eight
  bytes stay zero, and the fault does not move a byte.

The guest wrote that zero itself, into an out-parameter it expected filled.

### And the other wall answered the same way

PPSA28061 had the stack poison listed as untried for weeks. Tried: **byte-identical**.
Same fault, same `rdi=0x3` and `rax=0x9ba49`, same ten textures. Third class eliminated
there too.

So both walls, reached from opposite directions, now point at **an out-parameter nobody
wrote**. That convergence is worth more than either result alone.

### A dump now says which kind of nothing it found

A dump carried a bool - bytes or no bytes - so a count and an address pointing at nothing
rendered identically. `Pointing::{Scalar, Mapped, Unreadable}` now, classified against the
ranges already installed rather than a second constant. In the corrected run **nothing came
back `Unreadable`**, which disproves the guess I had written down beforehand: the pointer
was never out of bounds, the window was just wrong.

The CLI's two copies of the dump renderer became one on the way past, because the third
case would otherwise have had to be added to both.

### The pattern, again

Three edits this session all removed the same thing: one fact, written twice, where the
second copy drifted. `found_by` against the symbol database (D213), `all_dirs` against the
`paths` command (D215), and the stack span against the stack (D217). The third one had been
silently disabling the only tool that could see the biggest wall.

