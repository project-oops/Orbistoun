# D118 - The real library table, found by prediction rather than by guessing

**decided** · 2026-08-19

D117 established that library attribution was fabricated. This is the fix, and the method
is worth as much as the result.

**Do not guess which tag it is - dump them all and test a prediction that can fail.** The
correct table had to satisfy two things nothing else would: it must hold at least 55
entries (the observed library ids run 0..=54, and `DT_NEEDED` holds 52), and it must put
socket functions somewhere sane.

Two vendor tags in the OS-specific range carry `(id, name)` pairs:

| Tag | Entries | Indexed by |
|---|---|---|
| `0x61000049` | **55** | an import's library id, exactly `0..=54` |
| `0x61000045` | 52 | an import's module id, exactly `1..=52` |
| `DT_NEEDED` | 52 | nothing - a different list with different contents |

The value packs an id in the top sixteen bits and a string-table offset in the bottom
thirty-two.

**The prediction held.** `setsockopt`, `socket`, `bind`, `recv` and `pthread_mutex_lock`
all resolve to **`libScePosix`**; `printf`, `malloc` and `memcpy` to `libc`;
`sceKernelDirectMemoryQuery` to `libkernel`. Under the old mapping every one of the POSIX
calls sat in a graphics driver. `libScePosix` does not appear in `DT_NEEDED` at all, which
is the clearest possible sign the two lists are unrelated.

Nothing was read to find this. The file says what it contains; the work was deciding what
question to ask of it.

