# 2026-09-01 - sysctlbyname implemented; kern.osrelease answers (obSCEne osrelease passes)


Next obSCEne divergence (D446 list): `135-sysctl/osrelease` failed because `sysctlbyname` was unimplemented
and the stub refused - and a refused osrelease is what turns firmware detection off in a title that asks.
Implemented `sysctlbyname` + a pure `sysctl_value`: `kern.ostype`="FreeBSD" (the citable FreeBSD-derived
fact), `kern.osrelease`=the configured `machine().kernel_release` (empty until a profile sets it - not
invented; a console measured "0.0-prototype"), everything else refused (kern.version/hw.ncpu are a measured
banner and a core count orbistoun does not carry honestly). POSIX contract, never overruns the given length;
too-small buffer → ENOMEM (added `errno::NO_MEMORY`=12). Oracle: `135-sysctl/osrelease` → pass (as on
console); `135-sysctl/names` → partial (answers ostype+osrelease, honestly refuses the other two). Distinct
fails 5→4, no regressions. Unit test pins `sysctl_value`. Kernel/core tests pass, clippy clean, fmt-clean
(D447).

Left: `137-kernelcall/system-version` skips on hardware (its getpid gadget didn't resolve there) but fails
under orbistoun (getpid resolves, reaches the raw syscall) - a resolving/syscall divergence, not a knob.
Remaining fails: `110-modules` (one-module gap), `900-surface/control` (resolver reports a non-existent
symbol present).

