# 761. sceKernelFstat was unwired, and GTA's null-vtable is a runtime-init short-circuit

**2026-09-21** — inspecting the eboot's relocation tally settled worklog 760's two hypotheses (both
wrong), reframed the null-vtable wall, and turned up a real, honest fix along the way: a libkernel
name that was declared everywhere but here.

## The relocation tally rules out worklog 760

A temporary diagnostic printed the executable's `RelocationTally`:

```text
relocations 174172/174172 applied (0 weak-zero, 0 TLS-deferred, 0 unsupported, 0 unresolved)
```

Every relocation applied - the tally is `complete()`. So the null vtable at `image+0x5b37e98` is
**not** a skipped relocation, and the executable's `DT_INIT_ARRAY` is **not** truncated by an
unrelocated null (every init-array pointer relocated, so the crt's static constructors all ran). Both
of 760's candidates are dead.

That means the object is **not constructed by a static constructor at all**. It is the game-engine
pattern: a zero-initialised global whose vtable is written by an **explicit runtime `Init()`**, run in
a controlled order rather than at C++ static-init time. That runtime `Init()` is what was
short-circuited - which puts the run's stub calls back in play (760 set them aside "by ordering",
which only held for a static ctor).

## The fix that came off it: sceKernelFstat

Of the six calls landing on stubs, `sceKernelFstat` was both a prime suspect (an out-parameter call
writing **nothing**, the D171 failure) and - it turned out - a plain wiring gap. `fstat` (POSIX) and
`sceKernelStat` (libkernel) are both served, sharing one `struct stat` writer; the libkernel
`sceKernelFstat` name was simply never declared or registered, so a guest calling it read whatever its
stack held where the `struct stat` should be.

Wired it: `metadata::kernel_fstat`, the vendor-named form of `fstat` exactly as `kernel_stat` is of
`stat` (D525) - success and the `struct stat` shared through a new `facts_from_descriptor`, and only
failure differs (a descriptor with no file is `EBADF`, a bad buffer is `EFAULT`, in the
`0x8002_00xx` family a caller can test as negative, never POSIX `-1`). Declared in `libkernel_fs`,
registered beside `sceKernelStat`, knowledge recorded, and a test pinning the vendor-code-not-`-1`
contract.

**It moved the run FURTHER**: `+6 calls, +1 import` (`74 -> 75` distinct), the guest now doing real
work with a real `struct stat`. Honest, accurate emulation from the file's own metadata - no guess.

## But it is not the null-vtable's short-circuit

The fault is still `image+0x19676d7` - the same null-vtable virtual call. So `sceKernelFstat` was a
genuine gap and a genuine gain, but it is **not** what stops the object from being constructed. The
runtime `Init()` is still short-circuited by something else upstream. The remaining suspects are the
other five stubs the run hit - `sceUserServiceGetGamePresets` (out-param, D346-deferred),
`scePthreadGetaffinity` (D523-deferred), `sceImeUpdate`, `sceCoredumpRegisterCoredumpHandler`, and one
more - or a value fed wrong further back. That is the next dig: which one sits on the `Init()` chain
that constructs `image+0x5b37e98`.

## Gate state

Changed `crates/orbistoun-fs/src/metadata.rs` (`facts_from_descriptor` + `kernel_fstat` + a test),
`crates/orbistoun-fs/src/lib.rs` (declaration + registration), the `libkernel_fs` knowledge file, and
the regenerated `PROJECT_STATUS.md`/`COMPATIBILITY.md`/`compat-frontier.txt` (GTA at 75 imports,
`FURTHER`).

The full gate also surfaced a pre-existing cross-test race - `orbistoun-input`'s two `active_tests`
share the process-wide `ACTIVE` script static, and though the code comments claim they "run as one
test", they were two parallel `#[test]`s that install over each other. Serialised them on a `SERIAL`
mutex (the same fix `said` got in worklog 757); 47/47 pass multi-threaded now. It is the same class of
latent flake, unrelated to `fstat`, that a green workspace run exposes as coverage widens.

`./bin/orbistoun check` green, worklog index regenerated, identity scan clean. No commit.
