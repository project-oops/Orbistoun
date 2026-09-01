# D413 - Emulation of the kernel escape R/W pipe and dynamic symbol resolution


**measured** - 2026-08-31

Execution trace of `klog.elf` revealed that after CRT entry, the payload uses a 6-syscall loop body (`getpid` → `setsockopt(5)` → `setsockopt(6)` → `setsockopt(5)` → `setsockopt(6)` → `read(3)`) to read kernel structures via its socket/pipe escape primitive:
- `setsockopt(6, IPPROTO_IPV6, IPV6_PKTINFO, ...)` passes the targeted kernel memory address in its options buffer (`+0x04`).
- `read(3, dst_buf, len)` reads `len` bytes from the kernel address into userland.

Implemented the kernel read memory model in `crates/orbistoun-fs/src/escape.rs`:
- Captures targeted `kaddr` from `setsockopt` with `IPPROTO_IPV6`.
- Fulfills `read(3)` with simulated kernel structures:
  - `allproc` → pointer to `struct proc`.
  - `struct proc` → `p_ucred`, `pid` (matching `getpid()`), `p_dynlib` (pointer to `dynlib_obj` linked list).
  - `dynlib_obj` → module handle `0x2001` (`libkernel`), imagebase, and RTLD metadata pointer.
  - `RTLD_META` → `symtab` and `strtab` containing NID-encoded symbol mappings (including `sceKernelDlsym` -> `"HoLVWNanBBc"`).
- `descriptor::read` handles `fd == 3` using `escape::read_kernel_pipe`, allowing `kernel_dynlib_resolve` and `__crt_start` to complete dynamic symbol resolution and transfer execution to `main()`.




