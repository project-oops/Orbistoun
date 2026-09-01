# orbistoun-mem

The guest address space.

**Models:** fixed-address reservation with the ABI alignment and overlap rules
(`AddressSpace::validate`), region protection, and the direct/flexible memory
distinction.

**Deliberately fakes:** the mapping itself. `reserve` validates and then returns
`NotImplemented` - the platform primitives are unwritten.

**Design note.** Validation is separated from mapping on purpose, so the ABI rules
are fully testable without touching the host address space. This is the shape to
copy elsewhere in the codebase: a pure decision function plus a thin effectful
wrapper.

Only two platform primitives are needed:

- **Unix:** `mmap` with `MAP_FIXED_NOREPLACE`, which fails rather than silently
  evicting an existing mapping. Plain `MAP_FIXED` is never correct here - it would
  unmap host memory and the failure would look like guest corruption.
- **Windows:** `VirtualAlloc2` with a placeholder reservation, the only way to get a
  specific range with an explicit conflict error.

Reservation fails rather than relocating. A guest that asked for an address and got
a different one corrupts itself in ways that look like anything except a mapping bug.

**Status:** done and verified on both platforms (D055). Windows uses `VirtualAlloc` at
an explicit base - it never overwrites an existing reservation, so `VirtualAlloc2`
placeholders are unnecessary complexity until sub-dividing a reservation is needed.

Linux was **broken and only running it showed that**: `MAP_PRIVATE` was missing, and
every `mmap` error was being reported as a conflict, so an `EINVAL` read as "range
taken". tests, on both platforms. All guest memory access is confined to this crate - if a subsystem needs a
raw pointer, the abstraction is in the wrong place.
