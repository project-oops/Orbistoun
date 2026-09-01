# D190 - One allocation path, because alignment cannot be a special case


**decided** · 2026-08-21

Two Unity titles print `tlsf_create: Memory must be aligned to 8 bytes.` and then fail to
build their allocator. `memalign` was unimplemented, so it answered the placeholder error
code - and `0x7fff0001` is not eight-aligned, which is precisely what the guest complained
about. It was reporting the bug accurately the whole time (D186).

### Why not an aligned path beside the ordinary one

`dealloc` given a layout that differs from the one `alloc` received is undefined behaviour,
so the alignment has to survive from allocation to release. A separate aligned path would
need its own header that `free` could still read, and **the first time the two disagreed the
failure would be a heap corruption with no connection to either of them** - the worst
possible distance between cause and symptom.

So there is one `allocate(size, align)`, and `malloc` is it with the alignment set to the
header size. The block records `offset` - the distance from the allocation's start to the
pointer handed out - which equals the alignment the layout was built with, so one word
stores both facts: where to give the memory back, and what to tell `dealloc`.

The header was already sixteen bytes with eight in use, so this cost no space.

`header_of` refuses a block whose recorded offset is not a plausible one, which turns a wild
pointer into a no-op rather than a `dealloc` against a layout nobody allocated. The real
`free` has no such option; this one does, and declining is strictly better than corrupting a
heap it does not own.

### What it bought

With the abort-at-53 function overridden, the alignment complaint disappears and the guest
proceeds to an unrelated fault at `image+0xafc959`, with 96% of its calls on real
implementations.

**Not yet visible in an honest run.** Both titles still stop at 45 calls, before `memalign`
is ever reached, because `0x48a758b2e731cfd7` still answers an error. A fix that pays out
only behind another fix is worth recording as exactly that.

