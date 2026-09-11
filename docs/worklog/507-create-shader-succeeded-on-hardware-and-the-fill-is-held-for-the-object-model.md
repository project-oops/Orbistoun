# 507. create-shader succeeded on hardware, and the fill is held for the object model

**2026-09-11** - loop, on obSCEne's `20260910-214758-eboot.obs.log` (operator pointed me at it)

The shader wall's missing piece - measured. And a deliberate decision not to implement it yet.

## The breakthrough

obSCEne's Phase-6 sweep called `sceAgcCreateShader` with a **retail** argument shape (not the
synthetic well-formed shape that refused with `0x8a6c002f`, worklogs 505/D565) and it **succeeded**:

- `rc-retail 0x0`, `obj-valid 0x1`
- the two fields the guest reads at the D556 fault site, measured on a *successful* call:
  `+0x30 = 0` (quadword), `+0x50 = 0` (dword)
- `+0x10 = 0x6cd000` - an address (obSCEne's process, run-specific)

This is exactly what worklog 505 named as the blocker: "the fill needs measuring." It is now measured,
on real hardware, by obSCEne calling the actual library - external oracle, the safe kind. Recorded on
the `sceAgcCreateShader` knowledge entry, `known_by = measured`.

## Why it is not implemented yet - a new mechanism, and a moving target

Implementing it is **not a shim**. A handler is `fn(args) -> rax` and can only write guest memory it
is *handed* an address to (how the Gnm builders and pad-reads fill guest-supplied buffers).
`sceAgcCreateShader` must **produce** an object: `arg0` is a pointer-to-pointer (D556), so the call
allocates a region, writes its pointer into `*arg0`, and populates it. Nothing in orbistoun produces
guest-resident objects, and the current contract cannot. Doing it needs either a richer handler
contract or a reserved shader-object region in the address map - and the latter touches the
**loader/mem setup denied to this session**.

Two unknowns compound it: the object's size and *where it lives* (driver-heap vs guest-adjacent) are
unmeasured, and the operator's own **oops-sdk** is actively driving obSCEne into 3D/Phase-6
(`sceAgcDriverCreateQueue`, `SET_CONTEXT_REG`, shader stage differentiation) - so the object's shape
is still moving under any implementation.

## Decision: B then A (operator-chosen)

- **B:** filed `REQ-20260911T0930Z-3c5e` to obSCEne for the create-shader **object memory model** -
  the pointer written into `*arg0` and its distance from `arg0/arg1/arg2`, the object's extent, what
  `+0x10` points to, and the retail arg shape that succeeded vs the shape that refused. That one datum
  decides cheap-vs-subsystem: guest-adjacent object → a handler writes into memory the guest owns;
  driver-heap object → orbistoun needs the object-production mechanism it lacks.
- **A:** hold orbistoun code until that lands and the 3D work settles. No implementation written.

This keeps to the session's through-line: build against obSCEne/hardware (external truth), never
against our own apps (the home caution earlier today), and flag a new mechanism rather than assume it.

## Housekeeping

- The breakthrough Monitor went stale - obSCEne changed the section format (`rc-retail`/`shader-obj`
  vs `rc-wellformed`/`out-after`), so it did not fire on `214758`. Stopped it: a content-grep watch is
  not worth maintaining against a format the operator is actively changing, and the operator points me
  at the relevant files directly. Loop heartbeat remains the periodic check.
- Identity leak fixed earlier today (`orbistoun-elf/tests/inspect_test.rs` now derives from
  `%APPDATA%`; guard clean, staged).
- home (`oops-apps/home`) parked: our own code, not an oracle.

## Next

- On `3c5e`: if guest-adjacent, implement `sceAgcCreateShader` to write `{+0x30:0, +0x50:0, +0x10:ptr}`
  into a guest-owned region and return 0 - the first *measured* pass of the shader wall. If
  driver-heap, draft the object-production mechanism as a decision (contract vs reserved region), noting
  the loader-denied constraint, before any code.
- Otherwise hold; the operator's 3D work will keep producing object data.
