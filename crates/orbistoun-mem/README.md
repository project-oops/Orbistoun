# orbistoun-mem

The guest address space.

A guest module is linked to load at specific addresses, and its allocator hands out addresses
the guest's own code dereferences directly, so orbistoun reserves exactly what the guest
expects inside the host process. The crate holds fixed-address reservation with the ABI
alignment and overlap rules (`AddressSpace::validate`, `AddressSpace::reserve`), region
protection, the direct/flexible memory distinction, and the host primitives (`platform`).
Every crate that touches guest memory depends on it.

## Rules

- **Validation is separate from mapping.** The ABI rules are a pure decision function,
  testable without touching the host address space, with a thin effectful wrapper. This is
  the shape to copy elsewhere.
- **Reservation fails rather than relocating.** A guest that asked for an address and got a
  different one corrupts itself in ways that look like anything except a mapping bug.
- **Never evict.** On Unix, `mmap` with `MAP_FIXED_NOREPLACE`, which fails rather than
  silently evicting an existing mapping; plain `MAP_FIXED` would unmap host memory and the
  failure would look like guest corruption. On Windows, `VirtualAlloc` at an explicit base,
  which never overwrites an existing reservation.
- A host refusal is reported with its cause. An `EINVAL` is not a conflict, and reporting it
  as "range taken" hides the real fault.
- **Guest memory access is confined to this crate.** Everything above it uses safe, checked
  accessors; a subsystem that needs a raw pointer has the abstraction in the wrong place.
