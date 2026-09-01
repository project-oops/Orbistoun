# 2026-08-31 (later) - payload retry after further changes: 7 -> 32 syscalls, into klog's socket loop


A second retry of `prosperous/klog.elf` after more changes landed reached **32 syscalls**, up from
seven. Past its init (getpid, vendor_system_version 649) the payload now enters klog's socket-server
setup as a six-syscall loop body - `getpid -> setsockopt(5) -> setsockopt(6) -> setsockopt(5) ->
setsockopt(6) -> read(3)` - running about five iterations before jumping to `image+0x2708`
(illegal instruction), a target computed from a syscall return. Two observations for the next
session: the loop is a *retry* (the same setsockopt/read body repeats, so the payload is rejecting
what it gets back), and syscall 20 (getpid) returns inconsistent values across calls (`0x1`,
`0x2001`, `0x2`, `0x4000000181a8`) where a real getpid is stable - worth checking the syscall-20
handler. orbistoun's fault-address verdict still reads "same" because the wall address is unchanged
at `image+0x2708`; the fault-address metric does not see the 7->32 syscall growth, which is the
real movement here.

