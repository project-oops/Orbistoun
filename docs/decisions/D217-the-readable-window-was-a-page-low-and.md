# D217 - The readable window was a page low, and the wall had been unreadable because of it


**decided** · 2026-08-24 · found while working out what to do next

`orbistoun-worker` told the argument-dump path that guest memory was safe to dereference
across `(GUEST_STACK_BASE, DEFAULT_STACK_SIZE)` - **the two values it had passed to
`GuestStack::reserve`**, rather than the span the stack actually built. Those are not the
same. `reserve` puts a guard page at the base and starts usable memory one page above it,
so the real span is a page higher at both ends.

The declared window therefore:

- **offered the guard page**, which is mapped inaccessible on purpose, so a dump there
  would have faulted inside the emulator; and
- **refused the top page of real stack**, which is where a guest's initial frame lives.

### Why it mattered far more than an off-by-one usually does

`libkernel::0x6abac2f3dc6f8cee` is the lead on the `image+0xafc959` wall, and it is called
with exactly one pointer: `0x600000800d38`. That address is **712 bytes inside the page
that was excluded**. So `is_readable` said no, the dispatch path filed the argument as "not
a pointer", and it rendered as a bare number.

The technique that characterised the other two unnamed libc functions - dump the argument,
see what it points at (D194) - had never once been able to run against the thing on the
biggest wall. `docs/BACKLOG.md` said the next move was to *name* it; naming had been
exhausted (D213) and the argument was sitting there unread the whole time.

### The fix is to ask rather than re-derive

`GuestStack` already exposes `lowest_usable()` and `len()` and they were unused. The
install moved into `enter`, after the reservation, and now reads them.

It was in `prepare_diagnostics` because that runs *before* the stack exists - which is the
whole reason the constants were used instead. Worth naming as the cause rather than the
occasion: **one span, written twice, and the copy that was wrong was the one the diagnostic
used.** Same shape as D213 and D215.

### And a dump now says which kind of nothing it found

`ArgumentDump` recorded a bool: bytes, or no bytes. So a **count** and an **address
pointing at nothing this run mapped** rendered identically, as a bare number. That is
principle 3 in the tool being used to diagnose the wall.

It carries `Pointing::{Scalar, Mapped, Unreadable}` now, classified against the ranges
already installed rather than a second constant for where guest memory begins. The CLI
prints the third case as `-> no region this run mapped, and address-shaped`, through a
renderer that is now one function instead of the two copies it was about to become.

### What the run then said

With the window correct, the wall function dumps in full for the first time:

```
libkernel::0x6abac2f3dc6f8cee was called 1 times and has no name
  arg0 = 0x600000800d38 -> stack+0x800d38 = 00*8, 49 ba a2 01 00 00 00 00, 01*1, 00*8
  arg1 = 0x100000
  arg2 = 0x0
  arg3 = 0x40000
  arg4 = 0x0
  arg5 = 0x600000800db8 -> stack+0x800db8 = <image base>, 0x20, 0x10, 0x600000800ee0
```

The fault is `write to 0xfffe0` with `rdx = 0x100000`. **`arg1` is that same `0x100000`,
and `0x100000 - 0x20 = 0xfffe0`.** The guest asked for a megabyte, aligned to `arg3`
(256 KB), and then indexed to `base + size - 0x20` with a base of **zero**.

Two sources for that zero are now eliminated rather than argued about:

- **Not the stub return.** An unimplemented stub returns a placeholder error code, which is
  not zero, so the guest did not use the return value as the base.
- **Not unwritten memory.** Re-run with `ORBISTOUN_STACK_FILL=5a` (D185): the stack is
  `0x5a` throughout, `arg0`'s first eight bytes are **still zero**, and the fault does not
  move a byte. The guest wrote that zero itself.

Which leaves the reading that fits everything: `arg0` is an **out-parameter the guest
initialised to zero and expected the callee to fill**, and the callee is a stub that fills
nothing. Not proven - proving it means writing a recognisable value there and watching the
fault address follow - but it is the only surviving explanation, and it is the same class
as PPSA28061's remaining hypothesis: *a side effect nobody performed*.

**No pixel of this was reachable before the window was fixed.**

