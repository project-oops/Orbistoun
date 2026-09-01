# D417 - Three more HLE fixes from the same diff: thread join, mutex type, audio init


**measured** - 2026-08-31

Continuing D416 down the hardware-vs-orbistoun divergence list, each confirmed by the verdict
flipping when obSCEne re-runs:

- **`scePthreadJoin` carries the thread's return value back** (`orbistoun-kernel`,
  `orbistoun-kernel/thread`). `enter_guest_with_argument` already returns the guest thread
  function's `rax`; `spawn` now keeps it in an exit-value map and `join` makes it available once
  the host thread ends, so `scePthreadJoin` writes the real value through its out-parameter
  instead of a zero. `030-thread/join` partial -> pass.
- **Mutex type is honoured** (`orbistoun-kernel/sync`). `pthread_mutex_init` ignored the
  attribute's type and every lock was non-recursive, so a self-`trylock` always answered busy.
  It now reads the type the attribute already stored (`ATTR_TYPE`): 2 is recursive, 4 is
  error-checking. `try_lock` grew a third outcome - the owner re-taking a recursive lock is
  *locked*, a normal one is *busy*, an error-checking one is a *deadlock* - which
  `scePthreadMutexTrylock` maps to `0x0` / `0x8002_0010` / `0x8002_0016`, matching the console's
  three answers in `015-sync/mutex-recursion`.
- **`sceAudioOutInit` succeeds** (`orbistoun-audio`). The crate declared the audio surface but
  exposed no implementations, so every call including init fell to the placeholder's non-zero
  code and `090-audio/initialise` read a failed init. Init now answers success - the port calls
  that would move samples stay unimplemented, because there is no backend and pretending is the
  D171 shape.

`sceKernelCreateSema` (`018-relational/handle-fits`) was on the list and deliberately left alone:
its verdict differs because *hardware* fails it - the console writes past the end of the int -
while orbistoun writes a clean four-byte handle. orbistoun is the more correct of the two, and
matching the quirk would mean writing a bug.

Total across D416+D417 vs the pre-fix run: 516 -> 522 pass, 12 -> 8 partial, 8 -> 6 skip, fails
unchanged, plus the LoadStartModule and mutex measures now matching hardware. Recorded `measured`.

