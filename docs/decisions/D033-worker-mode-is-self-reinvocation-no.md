# D033 - Worker mode is self-reinvocation; no binary is privileged

**decided** · 2026-08-19

The child is not a separate executable and not "the CLI acting as host" - either
would make one shim special, which contradicts D034. It is a **mode** any shim can
enter: the binary re-invokes itself with a hidden flag and does nothing but host the
crates and speak the protocol over stdio. The pattern browsers use for their process
model.

Two concrete advantages over a distinct `orbistoun-worker` binary: no version skew is
possible (it is literally the same executable), and worker mode stays as thin as the
other shims.

**Both shims go through the worker uniformly.** The CLI gets no in-process fast path
just because a CLI crash is cheap - one execution path always, which means the GUI's
protocol is exercised on every CLI run. `--in-process` exists purely as a debugging
escape hatch (attaching a debugger through IPC is miserable) and is documented as a
dev aid, never a second supported mode.

