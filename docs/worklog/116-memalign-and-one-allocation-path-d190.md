# 2026-08-21 - memalign, and one allocation path (D190)


`memalign` was named this session from the repository's own word list, and implementing it
clears the `tlsf_create: Memory must be aligned to 8 bytes.` wall outright - the guest was
reporting the bug accurately, since the placeholder error code it received is not
eight-aligned.

Implemented as **one** `allocate(size, align)` with `malloc` as the case where the alignment
is the header size. A separate aligned path would need its own header that `free` could
still read, and the first disagreement between the two would be a heap corruption with no
connection to either - the worst possible distance between cause and symptom. The block now
records the offset from its start to the pointer handed out, which equals the alignment the
layout was built with, so one word stores both what `dealloc` must be told and where to give
the memory back. The header already had the space.

`header_of` also declines a block whose recorded offset is implausible, so a wild pointer
becomes a no-op rather than a `dealloc` against a layout nobody allocated. The real `free`
has no such option.

With the abort-at-53 function overridden, both titles reach an unrelated fault at
`image+0xafc959` with 96% of calls on real implementations. **Not visible in an honest run
yet** - they still stop at 45 calls, before `memalign` is reached, so this is a fix that
pays out only behind another fix.


