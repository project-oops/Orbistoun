# 621. A null-ish fault in orbistoun's own code is the guest's libc pointer, not an "EMULATOR BUG"

**2026-09-16** - the fault report called PPSA02664's wall an emulator bug; it is the guest handing a
libc shim an unpopulated pointer, and the report now says so - the same reframe `int 0x41` needed

## The misleading claim, and where it came from

PPSA02664 faults `read of 0xa8` inside `orbistoun_libc::memcpy`, and the report headlined it
**"EMULATOR BUG: the fault is in orbistoun's OWN code, not the guest's."** That is the wrong lesson,
in the exact shape this project keeps having to correct - the `int 0x41` "kernel entry" (worklog 611),
GTA's "missing asset" (worklog 603). The instruction pointer *is* in orbistoun's code, so the
"outside the guest image" test the message rests on is satisfied - but the address, `0xa8`, is null
plus a field offset, which is not an address any running code computes. It is a **dereferenced null
structure**: the guest handed `memcpy` a pointer it expected populated and orbistoun's `memcpy` -
which is correct - faulted on it, the same way the guest's own `memcpy` would fault on real hardware.
Sending a reader to debug orbistoun's `memcpy` is sending them to read correct code; the cause is
upstream, the call that should have filled the pointer.

## The fix: a third fault note, chosen by the address

The report had two notes for a fault whose IP is in orbistoun's code: the placeholder note (the guest
jumped through a `0x7FFF_0001` stub answer - not a bug) and the emulator-bug note (our code faulted).
A **null-ish faulting address** is a third case, and it now has its own note:

> `NULL-ISH ADDRESS IN OWN CODE: ... When the host stack below is orbistoun_libc, this is the guest
> handing an unpopulated pointer to memcpy/strlen - its own libc would fault the same on hardware, so
> the cause is upstream ..., not this emulator. When the stack is not a libc shim, it is an emulator
> null-deref after all.`

It is stated **conditionally**, because a null deref in orbistoun's own logic lands here too, and the
host stack (printed right below) is what tells the two apart - so the note points the reader at the
one piece of evidence that settles it rather than asserting a cause it has not checked (principle 3).
The decision is `is_null_pointer_fault(addr)` - pure, `addr < 0x1000` (the `NEAR_NULL` a page that the
diagnose crate already uses), so the threshold is tested without raising a real fault.

## Why this, this tick

The two remaining commercial walls (`int 0x41`, worklog 620; and this null-deref) both bottom out in
il2cpp internals that need reverse engineering, the GPU/translate work is a concurrent session's, and
the bus has no new obSCEne data to consume (its four newest requests are all outgoing). So the
highest-value move available was to stop the report misdiagnosing the one it can already see: the next
person to read PPSA02664's fault is now told it is a guest pointer with an upstream cause, not an
afternoon spent in orbistoun's `memcpy`.

## Made to fail

- `a_null_ish_fault_in_own_code_is_the_guests_libc_pointer_not_an_emulator_bug` (worker) -
  `is_null_pointer_fault` is true for `0xa8` and `0x0` and **false** for a real guest address
  (`0x740001f8f070`), so a genuine emulator bug is not relabelled away; and the two notes are distinct
  - the libc note points upstream and does not carry the "EMULATOR BUG" verdict, the emulator note
  still does. Verified live: PPSA02664's fault now prints the null-ish note over the `memcpy` stack.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` 2,361 pass / 0 fail, worklogs unique, identity scan clean. Frontier
regenerated for the session's record improvements.
