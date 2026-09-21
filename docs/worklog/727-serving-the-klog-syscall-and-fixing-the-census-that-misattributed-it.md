# 727. Serving the klog syscall, and fixing the census that misattributed it

**2026-09-20** — the AGC indirect-register wall (worklog 726) is blocked on obSCEne request 7c22, so
this tick took a different, non-hardware-blocked gap: a direct system call the census reported as
unserved. It served the call - and, more usefully, corrected the tooling that made the call look like
it belonged to a title it does not.

## The call: syscall 601 is the kernel-log write

`./bin/orbistoun run PPSA02664` ends with a census of direct kernel calls nothing implements, and it
listed `601` with first argument `7`. The number is identifiable from two lawful sources: FreeBSD's
generated table calls 601 `pdwait`, but the vendor kernel repurposes it, and this project's own
sibling source settles what to - obSCEne's `runtime.c` and `posix.c` issue
`obs_invoke_syscall(601, 7, buf, …)` in three places as their raw log-write channel, one of them
labelled `sys_call(SYS_klog, 7, marker_klog, …)`. So selector `7` writes the string at the second
argument to the operator log, the same one-function-two-answers shape D641 found when it bound
`sceKernelVirtualQuery`'s number.

The binding is served the way D641's was: a `SYS_vendor_klog = 601` entry in `vendor-syscalls.toml`
and a `SPELT_DIFFERENTLY` rename to `sceKernelDebugOutText`, whose implementation already writes the
second argument and ignores the first - a channel for the log call, a selector here, the same position
ignored the same way. A guest logging by the raw number is now served rather than answered `ENOSYS`
and dropped. A dedicated handler that gated on selector 7 was written first and then reverted: an
implementation in orbistoun must be a declared, guest-resolvable import (the registry-reachability
gate enforces it), and a syscall-only function that is no import cannot satisfy that without
fabricating an import name - a worse dishonesty than the selector simplification the alias inherits.

## The correction: the caller is not PPSA02664

The census sits at the end of `run <title>`, so `601` read as PPSA02664's. It is not. PPSA02664's own
freshly-written trace contains **no** syscall 601. The number comes from a different module the census
totals in: the `dist` homebrew payload (one of ours, built on the obSCEne runtime), whose trace holds
exactly one syscall - `{601, arg 7}`, unserved. The aggregate census merges every module's trace, so a
number in it belongs to whichever guest issued it, not to whichever title the command named. This is
the second time a probe's artefact was taken for a retail title's wall (worklog 721 was the first, over
a `todo:` print), and the reason is the same: a per-title command printing a cross-title total.

## The tooling fix, so it does not happen a third time

`report_kernel_calls` now prints an `ASKED BY` column naming the guest each direct syscall came from,
built from a new `guest_label` that shortens a module path to the eboot's own directory (`dist`,
`PPSA02664-app0`). The census now reads:

```
  CALL   RUNS  FIRST ARGUMENT  ASKED BY
   601      1  0x7             dist
```

which states outright what a reader otherwise had to know: this call is `dist`'s, not the title's. The
guard is `a_guest_is_labelled_by_its_own_eboot_directory`, and the case it pins is the one that matters
- a probe path resolving to `dist`, never to the title the command happened to name.

## Honest disposition

Two things moved, neither of them PPSA02664's wall. A real vendor syscall is served where it was
refused, correct and citable, for the guests that actually issue it. And the census that misattributed
it now attributes every direct syscall by guest, which is worth more than the one binding: it removes a
whole class of "the title is at fault" misreadings that D708 exists to catch, the same class this tick
briefly fell into before the trace files were checked. Syscall 601 still shows unserved in the census
because `dist`'s trace is stale from before the binding and `dist` is not in orbistoun's run corpus to
re-run; the binding itself is proven by `the_vendor_klog_syscall_binds_to_the_log_write`, and `dist`
will be served the next time it runs through orbistoun.

## Gate state

`crates/orbistoun-hle/data/vendor-syscalls.toml` gains the constant; `orbistoun-service/src/symbols.rs`
the rename and its binding test; `orbistoun-cli/src/main.rs` the `guest_label` helper, the `ASKED BY`
column, and its test. The dedicated-handler attempt in `orbistoun-fs` and its knowledge entry were
reverted to net-zero. `./bin/orbistoun check` green; identity scan clean. No commit.
