# 2026-08-31 - obSCEne's hardware run absorbed: five vaddrs confirmed, and a payload retry


obSCEne's `139-exports` ran as a payload on the console and confirmed seven of eight candidate
export vaddrs by behaviour (D410). Promoted the five newly-confirmed ones - getuid `0x630`,
geteuid `0x650`, getgid `0x870`, getppid `0x7d0`, sceKernelGetProcessTime `0x16160` - from
candidate to `confirmed` in `data/libkernel-vaddrs.txt`; `libkernel_provenance` now reports them
Confirmed. sceKernelGetTscFrequency `0x1cf30` was refuted but stays a candidate: the offset is a
real export-table placement, and what failed was the probe's no-arg-getter assumption, not the
address. Removed a duplicate `getpid 0x5b0 confirmed` line.

Retried payload support against `prosperous/klog.elf`, which exercised the whole path end to end.
Plain `run` enters with argc in rdi (elfldr's "call address 1") and dies at `0x1`. Under
`--profile ps5-cex-12.40` (firmware skeleton maps at `0xf000000000`, base handed at `0xf040000000`)
plus `ORBISTOUN_ENTRY_ARGUMENT=handoff`, the payload reads `payload_args[0]=getpid`, computes its
base, and issues seven syscalls - getpid (20) and vendor_system_version (649) through the getpid+10
gadget - before hitting a new wall: an illegal instruction at `image+0x2708`, a computed jump from
a syscall return. The fault moved from `0x1` to `image+0x2708`. As expected, the provenance edits
are metadata and did not change how far the payload got; this was a no-regression confirmation, and
orbistoun correctly declined to record it as title progress because it ran under the handoff
diagnostic.

Two build notes for the next session. The workspace does not compile on Linux/WSL right now:
`orbistoun-mem` calls `rustix::param::page_size()` but the workspace rustix dependency enables only
`mm` and `thread`, not `param` (Cargo.toml is mid-edit, `AM`, so this was left for whoever is
editing deps). The `rustix::param` call is under `#[cfg(unix)]`, so the native Windows build is
unaffected - that is where this run was built and executed. Second, `--profile` prints
"ORBISTOUN_MACHINE_PROFILE is not a diagnostic this build understands - ignored" yet the firmware
still maps, so the profile takes effect through the service layer while the env passthrough warns;
worth reconciling but not blocking.

