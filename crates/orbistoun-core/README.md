# orbistoun-core

Domain types shared by every other crate: the bottom of the dependency graph. It holds no
behaviour beyond conversion.

It defines guest-visible error codes (`GuestError`), opaque handles (`Handle`,
`HandleAllocator`), the title `Category` and what it permits, and the fixed ABI constants
(`GUEST_PAGE_SIZE`, `DIRECT_MEMORY_ALIGN`).

## Rules

- Error codes are modelled here, never written as integer literals at a call site. A stub
  returning the wrong code is the commonest cause of a guest hanging thousands of frames
  later.
- Placeholder codes sit in a reserved range, `0xF7FF_0000`, that no real firmware value
  occupies, so a placeholder is never mistaken for an established value (D670).
- Handles are per-subsystem and never recycled. Reuse makes a stale-handle bug look like a
  valid access to the wrong object, which is harder to diagnose than running out.
- The crate takes no runtime dependency. Anything that needs one belongs a layer up.
