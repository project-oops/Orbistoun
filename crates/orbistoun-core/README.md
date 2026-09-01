# orbistoun-core

Domain types shared by every other crate. The bottom of the dependency graph.

**Models:** guest-visible error codes (`GuestError`), opaque handles (`Handle`,
`HandleAllocator`), and the fixed ABI constants (`GUEST_PAGE_SIZE`,
`DIRECT_MEMORY_ALIGN`).

**Deliberately fakes:** nothing. There is no behaviour here beyond conversion.

**Design note.** Error codes are modelled centrally rather than sprinkled as
integer literals, because a stub returning the wrong code is the most common cause
of a guest hanging thousands of frames later. Placeholder codes deliberately avoid
the high bit, so they can never be mistaken for established firmware values.

Handles are per-subsystem and never recycled: reuse makes a stale-handle bug look
like a valid access to the wrong object, which is far harder to diagnose than
running out.

**Status:** complete for what exists above it. Adding a runtime dependency to this
crate is a smell - push it up a layer.
