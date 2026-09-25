# 871. Watchpoints follow every guest thread

**2026-09-25**. Three write watchpoints over PPSA25872's binding table (`0x74000b2f9d00`,
`+0x28`, `+0x48`) all reported `never touched`. The table was plainly written: it holds pointers.

**Cause.** Debug registers are per thread, and `watchpoint::arm` set them only on the thread that
became the guest. The table is built on a spawned thread, so every access from it went unseen.
`never touched` was an arm that could not fail.

**Change.** `arm` keeps the requests. `arm_this_thread` sets them on the calling thread, and the
per-thread start hook (`set_up_this_threads_tls`) calls it first. A thread that cannot be armed
says so (`a spawned guest thread runs unwatched`) rather than running as if it were watched.

Rerun, all three hit. Two writers:

- the C runtime's `memcpy` (host code, `0x7ff9c071…`) copies the header in: `0x38` at `+0x00` is a
  relative offset;
- orbistoun's own `sceAgcCreateShader` (`0x7ff7303c…`) relocates it to `0x74000b2f9d38`.

So the table is a shader header as shipped. Its slot-table-0 count of 2 with every slot `0xffff`
is the title's own data, relocated as hardware relocates it (worklog 529).

**Which moves the wall's question.** The upload at `image+0x3b37d0` asks for slot 2 of that
two-entry table, gets `0x7fff`, and writes to `[r14+0x18] + 0x7fff*4 - 0x80`. That is an
extended-entry buffer of `0x8000` entries, and on hardware it must exist. Here it is null. Next:
who allocates `[r14+0x18]`.

**And the caller narrows it further.** `image+0x11bd380` walks the set bits of a mask at
`[[rbp-0x38]+0x10]` and uploads each slot through `image+0x3b37d0` with no range check. The loop
just before it (`image+0x11bd312`) makes the same lookup and skips a slot that answers `0x7fff`.
So on hardware that mask never names a slot past the shader's count, and here it names slot 2.
The question becomes who builds that mask, more than who allocates `[r14+0x18]`.

**Limit, stated.** The start hook is installed only for a title with thread-local storage. A title
without TLS still watches its first thread alone. Every retail title here has TLS.
