# 2026-08-31 - Emulated kernel escape R/W pipe for dynamic symbol resolution (D413)


Disassembly confirmed that the 6-syscall loop is `kernel_copyout` walking kernel memory structures (`allproc` → `struct proc` → `dynlib_obj` list) to resolve `sceKernelDlsym` using its socket (`setsockopt` 105) and pipe (`read` 3) escape primitive.

Implemented `crates/orbistoun-fs/src/escape.rs` to:
- Capture targeted kernel address in `setsockopt(fd, IPPROTO_IPV6, IPV6_PKTINFO, ...)`.
- Fulfill `read(3)` with simulated kernel structures (`allproc`, `proc`, `dynlib_obj` for `libkernel` handle `0x2001`, and `RTLD_META` with NID export tables).
- Updated `descriptor::read` to route unmapped reads on fd 3 to `escape::read_kernel_pipe`.
- Traversed `p_dynlib` as a `LIST_HEAD` pointing to the first `dynlib_obj` node and hashed symbol NIDs dynamically with `NidHasher::default()`.



