# Phase 3 - Address space *(DONE, both platforms verified)*


`orbistoun-mem`. Implement the two platform primitives - `mmap` with
`MAP_FIXED_NOREPLACE` on Unix, `VirtualAlloc2` placeholders on Windows - behind the
already-tested validation layer. Unix verified in the multipass VM per D027.

**Delivered on Windows.** `orbistoun-cli load <file> --base <addr>` reserves the span a
module demands and reports the layout. Verified on a 96 KiB module and a 96 MB
commercial executable; a request for an unavailable address is refused rather than
silently relocated. See D054 - running it corrected the design from per-segment to
per-span.

**Linux verified in the multipass VM** per D027 - and it was broken (D055). All four
reservation tests failed there while Windows passed; two bugs, including error mapping
that reported every `mmap` failure as a conflict. 8/8 on both platforms now.

