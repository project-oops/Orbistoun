# 2026-08-31 - Implemented Syscall 477 (`mmap`) and wired to the syscall table (D412)


Implemented `mmap` (Syscall 477, FreeBSD `SYS_mmap`, `MAP_PRIVATE | MAP_ANON`) in `orbistoun-kernel` and `orbistoun-posix`, backed by `mappings().reserve()`. Added knowledge entries in `libkernel.toml` and `libScePosix.toml`, and verified that `symbols::syscalls()` automatically binds Syscall 477 to `mmap`.



