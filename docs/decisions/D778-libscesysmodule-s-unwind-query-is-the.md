# D778 - libSceSysmodule's unwind query is the kernel's

**Status:** decided
**Date:** 2026-10-09
**known_by:** assumed (the names match; nothing on the target compares the two)

`sceSysmoduleGetModuleInfoForUnwind(address, flags, info)` answers exactly as libkernel's
`sceKernelGetModuleInfoForUnwind` does (D768). It fills the same block of the module holding
`address`, and refuses as it refuses. The implementation is the kernel's function, registered
under the second name.

**Why:** PPSA02664's libc asks the sysmodule spelling 1100 times a run, and every call landed on a
stub. The two names differ only in library, and the census found both on the system. A copy
of the kernel's semantics would be a second place to keep them true.
