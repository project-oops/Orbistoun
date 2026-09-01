# 2026-08-31 (later) - three more HLE fixes: thread join, mutex type, audio (D417)


Crunched the rest of the hardware-diff list. scePthreadJoin now carries the thread's return value
(spawn keeps the guest function's rax, join hands it back); mutexes honour their type, with a
three-state try_lock so recursive/normal/error-checking give the console's three codes
(0x0/0x8002_0010/0x8002_0016); sceAudioOutInit answers success (the crate had declarations but no
implementations() - added one and wired it into service::symbols). Each verdict flipped on re-run.
sceKernelCreateSema left alone: orbistoun writes a clean 4-byte handle where hardware overruns the
int, so orbistoun is the correct one and matching the quirk would be writing a bug.

Surprise worth recording: changing sync::try_lock from Option<bool> to Option<TryLock> broke a
dozen test assertions across sync.rs's inline tests and tests/sync.rs - the compiler found them all,
and the inline module needed TryLock added to its `use super::{...}` list. Added a test for the new
Errorcheck path (a self-relock reports Deadlock, distinct from Busy). All kernel/audio tests pass;
my code is clippy-clean (the only -D warnings failures are orbistoun-fs's escape.rs/socket.rs, the
parallel workstream's). Note sync.rs is being edited by that parallel session too - the harness
flagged it changed mid-edit - but only in areas away from these changes.

Out of clean diff-driven fixes now. What remains diverging is either correct-as-is (GetModuleInfo,
which hardware also refuses), architectural and risky (900-surface/control - orbistoun's
stub-everything resolver answering a deliberately-absent symbol), the parallel escape workstream's
(137-kernelcall), or environment (ps4_mode-only libs, host-libc artifacts). Those want a person's
call, not an unattended crunch.

