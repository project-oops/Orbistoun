# 2026-08-31 - Disassembled `klog.elf` around `image+0x2708`: kernel_copyout, setsockopt, and high-half kpipe_addr (D411)


Investigated the wall at `image+0x2708` in `klog.elf`. Disassembly of the binary revealed:
- `__crt_start` calls `kernel_dynlib_dlsym(-1, 0x2001, "sceKernelDlsym")` (and fallback `kernel_dynlib_dlsym(-1, 2, "exit")`).
- `kernel_dynlib_dlsym` calls `kernel_dynlib_resolve`, which calls `kernel_dynlib_obj(-1, ...)`.
- `kernel_dynlib_obj` calls `kernel_get_proc(-1)` and `kernel_copyout` to read `proc->p_dynlib` at offset `+0x3e8` in kernel memory.
- `kernel_copyout` validates that `kpipe_addr >> 48 != 0` (requiring a canonical high-half kernel pointer, e.g. `0xffff86615c607840`), uses `setsockopt` (Syscall 105) on the `rwpair` sockets with `IPPROTO_IPV6` (0x29) and `IPV6_PKTINFO` (0x2e), and performs `read` on `rwpipe[0]` to read kernel structures into userland.
- Because `kpipe_addr` had been a low-half pointer (`FIRMWARE_BASE + offset`), `kernel_copyout` bailed with `EFAULT` (14) before attempting any syscall, causing `kernel_dynlib_dlsym` to return NULL and `__crt_start` to jump to `ud2` at `0x2708`.

Fixed `measured_handoff_fields` in `crates/orbistoun-worker/src/lib.rs` to hand over the measured canonical high-half addresses `0xffff86615c607840` (`kpipe_addr`) and `0xffffffff8c290000` (`kdata_base_addr`) as confirmed in D408.

