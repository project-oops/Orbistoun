# 2026-08-31 (later) - four HLE fixes from the hardware diff (D416)


Diffed the latest obSCEne hardware run (module build, fw 12.40) against the same suite in orbistoun
and worked the concrete divergences, each fix confirmed by re-running obscene.elf in orbistoun and
watching the verdict flip:

- `sceKernelWrite(_,_,0)` -> 0 (was InvalidArgument; guest_slice rejects zero length). 000-boot
  partial -> pass.
- `rand`/`srand` implemented with an atomic LCG (was an unimplemented stub answering a constant).
  035-libc partial -> pass.
- `sceKernelGetSystemSwVersion` fills the version struct with 12.40 (was refused). 130-layout
  partial -> pass.
- `sceKernelLoadStartModule` returns real handles by path (libkernel 0x2001, /app0 -> handle,
  /system -> 0x8002_0002), was -1 for all. 110-modules/load 0x0 -> 0x3, matching hardware.

Net vs the pre-fix run: 516 -> 520 pass, 12 -> 10 partial, 8 -> 6 skip, fails unchanged.

Two surprises worth carrying. First, the honest-failure catch: the first LoadStartModule cut handed
a handle to any non-system path, and 060-module/load-rejects-missing (which loads a bogus path)
promptly reported a nonexistent module as loaded - a stub returning plausible success. Recognising
only libkernel/`/system`/`/app0` and refusing the rest fixed it. Second, two diff divergences that
looked like bugs were not: GetModuleInfo refuses on hardware too (110-modules/names fail 0x8002_0016
this run), so orbistoun's refusal is correct; and the TSC frequency's 79 Hz difference is
boot-calibration variance, with the real lesson being that obSCEne's 139-exports exact-match is too
strict (an obSCEne fix).

Gate note: kernel/libc/fs-lib.rs changes are clippy-clean and all crate tests pass. A whole-tree
clippy still cannot pass - orbistoun-fs's escape.rs (D413) and socket.rs (AM) trip unreadable-literal
/ too-many-lines / unsafe-ops lints - but those are the parallel escape workstream's, not this one's.

