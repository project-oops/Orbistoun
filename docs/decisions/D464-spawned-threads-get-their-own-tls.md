# D464 - Spawned guest threads get their own thread-local storage


**measured** - 2026-09-02 (user-directed /loop: overnight oracle-free crunch)

Only the **main** thread had thread-local storage. `install_main_thread_tls` builds it a block,
installs its `fs` base and registers it with the context-switch backstop (D433) - but a thread the
guest spawned got none of that: `orbistoun-kernel`'s spawn body reserved a stack and entered guest
code, and the new thread's first `fs:`-relative access read a zero base and faulted. PPSA04263 died
this way at `image+0x2ba47bc` (`mov rax, fs:[0]` on a thread whose stack was `0x6100…`), and the
fault reporter's register-base guess could not name it, because the access is segment-relative and
naming it would mean disassembling the guest (worklog 286).

The fix is the call-budget inversion (D238 shape). The kernel spawns threads but cannot build a TLS
block - it holds no template and does not depend on the loader - so a `fn()` hook
(`thread::install_thread_start`) is called at the top of every new guest thread, on that thread,
before any guest code. The worker installs it. The effectful half of the main-thread setup was
extracted into `build_tls_block(base, layout, tdata)` - reserve, `render_block`, install the `fs`
base, `remember` - and the main-thread path now also stores the template `(TlsLayout, tdata)` so a
spawned thread can build its own block from it, at its own arena (`THREAD_TLS_BASE`, `0x6A00…`,
clear of the stacks, the main block and the mapping arena). No template - a title with no
thread-locals - and the hook is never registered, so nothing changes for those.

**Verified, two observations.** PPSA04263 went from 10,123 calls to **332,914** (`+322,791`, `+34`
distinct imports), faulting far later at `image+0x196b91a`: a program cannot make three hundred
thousand more calls without the thread whose first instruction used to fault now running. No
regression - PPSA02664, PPSA03416, PPSA21564 and PPSA25872 are unchanged. The block, base install
and backstop registration all run per-thread, so the Windows `fs`-reset (D433) is handled for a
spawned thread exactly as for the main one: `remember` sets a thread-local, and the fault handler
restores that thread's own base.
