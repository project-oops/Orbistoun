# Coverage crunch: kernel and thunk


67 more tests, in three new files under `tests/`.

| file | before | after |
|------|--------|-------|
| `orbistoun-kernel/src/sync.rs` | 47.76% | 94.24% |
| `orbistoun-kernel/src/lib.rs` | 38.31% | 74.74% |
| `orbistoun-thunk/src/dispatch.rs` | 49.03% | 84.81% |

### One real bug

`scePthreadRwlockUnlock` shared the `acquired()` helper with the four *acquiring* calls, so
releasing a lock nobody held answered **`Busy`** - the code meaning "somebody else has it".
The release path has no contention branch, so that can only ever be a bug in the guest, and
a guest reading `Busy` retries. `scePthreadMutexUnlock` beside it already answered
`InvalidArgument` for the identical situation. Given its own mapping, and the reasoning
recorded on it: a message naming a cause has to come from the branch that determined it.

### Surprises

**The process-global tables decide the test structure, not preference.** `orbistoun-thunk`
keeps every table in a `OnceLock` because a thunk has no register left to carry a context
pointer, so a second install is a silent no-op. That means one configuration per *process*,
which is why the new tests are a second integration binary rather than more tests in the
existing one, and why the stateful half is a single long function. Recorded because the
obvious tidy-up - splitting it into several `#[test]`s - would break it silently rather than
loudly.

**A coverage run after the move reported every file twice**, once under the old path at 0%
and once under the new one. Stale `.profraw` from before `<OOPS>/orbistoun` became
`<OOPS>\orbistoun`; `cargo llvm-cov clean --workspace` clears it. Worth knowing because
the stale rows look exactly like a crate nothing tests.

### Where the remaining gap is

`orbistoun-gen` is the largest uncovered crate left, and it is deliberately not being
chased: it is a binary-only offline generator, described in its own manifest as *not part of
the emulator*. Coverage there measures a tool that produces data files, not anything a guest
reaches.

