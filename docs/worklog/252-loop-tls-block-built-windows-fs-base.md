# 2026-09-01 (/loop) - TLS block built; Windows fs-base limitation found; knowledge debt cleared


(loader) `TlsLayout::render_block` (pure, tested) lays out a thread's storage variant-II with the
self-pointer at the thread pointer; `tls::layout_of` now returns the PT_TLS vaddr too (one caller
updated). (worker) `install_main_thread_tls` reserves a block, copies the `.tdata` the loader placed at
`image base + vaddr`, installs and reads back the `fs` base before entry.

SURPRISE (measured): the install works and reads back correct, but the guest still faults `fs:[0]`=0 -
Windows resets the user `fs` base to zero on the next context switch (a base reads back as 0 after a 2 ms
sleep). So install-once is a Linux solution; Windows needs a fault-handler re-install backstop. PPSA28061
therefore not advanced yet; foundation + finding recorded (D433).

(hle/service) Cleared accumulated knowledge/accounting debt so the tests go green: 30 implemented
functions documented (threading family, sceKernelVirtualQuery, video-out flips, GNM dispatch, sysmodule,
audio init, rand/srand), 3 new library knowledge files + EMBEDDED, mmap found_by corrected to the audit,
SERVES_NOTHING emptied (GnmDriver/AudioOut now serve). Loader/worker/service/hle tests green;
orbistoun-worker clippy-clean. Titles: no fault-site regression (PPSA02664 -1 import is timing noise from
the now-real _Xtime/_Thrd_sleep).

