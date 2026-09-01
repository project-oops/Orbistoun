# D072 - First names, and the first concrete implementation target

**decided** · 2026-08-19

The whole chain now runs end to end, on real material and with no external input:

| Executable | Imports | Named | Share |
|---|---:|---:|---:|
| 2 MB | 715 | 83 | 11.6% |
| 96 MB | 1,380 | 161 | 11.7% |

Both halves contribute. Published C names resolve the C library outright; the
combinatorial generator produced real vendor names that the hash then confirmed -
`sceCommonDialogInitialize`, `sceSaveDataInitialize3`, `sceSysmoduleLoadModule`,
`sceVideoOutSetBufferAttribute2`, and dozens more. Nothing was consulted to get them.

**And the one that mattered.** The function the 96 MB executable calls 431 million times
in ten seconds, previously known only as `libkernel.prx::0x7dd1e10c2d2e7a04`, is

> **`sceKernelDirectMemoryQuery`**

The guest asks about direct memory, is told "unimplemented", and asks again forever.
That is the wall, it has a name, and implementing it is now a specific piece of work
rather than a research question.

**Eighty-eight percent are still unnamed**, and that is a vocabulary problem, not a
method problem. Extending `data/vendor.toml` is the work, and it needs no rebuild.

