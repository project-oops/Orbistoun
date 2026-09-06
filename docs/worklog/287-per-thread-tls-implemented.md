# 2026-09-02 - (/loop) Per-thread TLS for spawned guest threads: PPSA04263 10k -> 333k calls

Implemented the fix designed in worklog 286: spawned guest threads had no thread-local storage, so
their first `fs:[0]` read a zero base and faulted. Followed the call-budget inversion (D238):
`orbistoun-kernel/thread.rs` gained a `ON_THREAD_START: OnceLock<fn()>` hook, called at the top of
every spawned thread (after `become_thread`, before `enter_guest`, so it runs on the new thread
where the `fs` base is per-thread). The worker extracted `build_tls_block(base, layout, tdata)` from
`install_main_thread_tls` - reserve, `render_block`, install the base, `remember` - stores the
template when the main thread sets up, and registers `set_up_this_threads_tls` as the hook, which
builds a block for the calling thread at a fresh `THREAD_TLS_BASE` (`0x6A00…`) arena. Recorded D464.

Verified two ways, and the result is large: PPSA04263 went 10,123 -> **332,914 calls** (`+322,791`,
`+34` distinct imports), faulting far later at `image+0x196b91a`. No regression - PPSA02664/03416
(1,544, `_Getpctype` wall), PPSA21564 (500,257) and PPSA25872 (39,929) all unchanged. `cargo
test`/`clippy`/`fmt` clean on kernel, worker and mem (the one clippy note is the untouched
pre-existing `enter` too_many_lines; I kept my hook registration out of it by putting it in
`install_main_thread_tls`, so it stayed at 113 rather than growing).

Two things worth keeping. (1) The registration lives in `install_main_thread_tls`'s success path,
not `enter`, so it fires only for a title that actually declares thread-locals - the right scope and
it avoids touching the over-long function. (2) PPSA28061 was unchanged despite the code comment
naming it as a past `fs:[0]` victim - its current wall is elsewhere (961 calls, GPU-shaped), so that
comment is now stale for it; not worth chasing tonight.

Next oracle-free: PPSA04263's new wall `image+0x196b91a` (333k calls in - deep in the guest now) and
PPSA25872's `0x7b594e`, both with the return column. The `scePthreadAttrSet*` advisory setters remain
easy stub-reduction when convenient.
