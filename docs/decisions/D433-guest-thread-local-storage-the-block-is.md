# D433 - Guest thread-local storage: the block is built, and Windows will not keep the base


**measured** - 2026-09-01 (user-directed, /loop)

Built the foundation the furthest title's `fs:[0]` wall (D432) needs: `TlsLayout::render_block` lays out
one thread's storage variant-II (the `.tdata` init image at the bottom, `.tbss` zeroed, the self-pointer
written at the thread pointer so `fs:[0]` reads the pointer), `tls::layout_of` now also returns the
`PT_TLS` vaddr so the runtime can copy the init image the loader already placed at `image base + vaddr`,
and the worker reserves a block, renders it and installs the `fs` base on the main thread before entry,
reading it back to confirm. All pure parts are tested.

**And then measured the wall the mechanism itself hits on this host.** The install works and reads back
correctly (`fs` base set to the block, read back equal) - but the guest still faults `mov rax, fs:[0]`
reading zero, because **Windows resets the user `fs` base to zero on the next context switch**. Measured
directly: a base written and read back correctly reads back as `Some(0)` after a 2 ms sleep. This is the
platform difference that matters - a Linux kernel with `FSGSBASE` saves and restores the real `fs` base
across context switches, so install-once holds for the run; Windows manages the base itself (its TEB is
in `gs`, `fs` is normally zero) and does not preserve a user `wrfsbase`. So the thread-pointer mechanism
the loader has always had (D061) is correct and sufficient on Linux, and on Windows it primes a base that
a fault-handler backstop must re-install whenever a guest `fs:` access faults on the zero it reverts to.
That backstop is the next step, recorded rather than pretended: the block, the layout and the priming are
the shared foundation; the Windows-specific re-install is not yet built.

Also this pass, the fault report grew the two windows of instruction bytes (D432) that made every wall
above disassemblable, and the knowledge and library-accounting debt that had accumulated across sessions
was cleared: thirty implemented functions that had no knowledge entry now have one (the C-runtime
threading family, `sceKernelVirtualQuery`, the video-out flip calls, the GNM dispatch builders, the
sysmodule and audio-init shims, `rand`/`srand`), three new library files carry them, `mmap`'s `found_by`
was corrected to match the audit, and `libSceGnmDriver`/`libSceAudioOut` left the serves-nothing list
they had outgrown. The service and hle knowledge tests are green again.

