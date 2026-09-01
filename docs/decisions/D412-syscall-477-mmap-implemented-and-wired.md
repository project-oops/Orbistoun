# D412 - Syscall 477 (`mmap`) implemented and wired to the syscall table


**published** - 2026-08-31

In open-toolchain payloads, `kernel_dynlib_resolve` mmaps a temporary scratch buffer using Syscall 477 (FreeBSD `SYS_mmap`, `MAP_PRIVATE | MAP_ANON`) to hold module export headers while resolving symbols.

Previously, `mmap` was not present in `implementations()`, causing `orbistoun-service`'s harvested syscall table to miss 477 and answer `-ENOSYS` (`0xffffffffffffffff` / `MAP_FAILED`), aborting `kernel_dynlib_resolve`.

Implemented `mmap` in `crates/orbistoun-kernel` using `mappings().reserve()`, declared `"sceKernelMmap" => 6` in `libkernel`, declared `"mmap" => 6` in `libScePosix`, and delegated `("mmap", "mmap")`. This automatically binds Syscall 477 to `mmap` in the harvested syscall dispatch table.

