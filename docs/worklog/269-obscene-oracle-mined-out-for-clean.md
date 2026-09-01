# 2026-09-01 - obSCEne oracle mined out for clean fixes; pivot to a real title next


Characterised the four remaining obSCEne failures under orbistoun and found none is a clean, safe fix (D448):
`900-surface/control` is the deliberate loud-stub-everything model (D392) - a correct narrow fix exists
(refuse weak+unknown imports, matching hardware's weak-undefined-null) but needs an ELF binding decode +
service refuse-set and re-baselines every measurement, so it is a deliberate deferred step, not a loop flip;
`110-modules/info-size` and `/names` need the `SceKernelModuleInfo` layout obSCEne itself says it does not
fully know (uncitable, D008); `137-kernelcall/system-version` skips on hardware (getpid gadget) and is the
same resolve-everything model plus honest ENOSYS on a raw syscall. Five real fixes came out of this oracle
(D443-D447 + flexible); the rest are design or layout-gated. Next frontier is a real title: PPSA02664 runs
1541 calls and walls in `libc::_Getpctype` (the ctype-table accessor), whose table FreeBSD documents.

