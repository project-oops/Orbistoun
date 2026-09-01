# orbistoun-kernel

libkernel - memory syscalls, threads, and synchronisation. Third in the dependency
spine.

**Models:** the direct-memory syscalls, thread creation and join, mutexes, mutex
attributes, and semaphores - fifteen functions with real behaviour behind them.

**Deliberately fakes:** the rest of the pthread surface, and everything about
scheduling. A guest has never called `scePthreadCreate` yet, so the threading path is
written and unexercised.

**Design note.** FreeBSD is the reference. The target kernel is
FreeBSD-derived and a large fraction of libkernel is POSIX with the vendor naming, so most
functions here have a documented, lawful, citable analogue - name it in a comment
when you implement one. This crate should need less guesswork than any other, and
if it does not, the analogue has not been looked for.

Guest threads must be **real host threads**. A green-threaded or pooled
implementation cannot work: guest code reads thread-local storage directly and
blocks in its own primitives.

**Status:** fifteen functions implemented. Mutexes and semaphores are built and
exercised - one title constructs eleven mutexes during static initialisation - but every
guest is still in single-threaded startup, so `docs/ROADMAP.md` phase 5 is begun and
nowhere near its own observable result.

One wall lives here: a title spins on `sceKernelDirectMemoryQuery`, walking the memory
map, refusing what it is shown, and starting again. The map shape it will accept is the
highest-ranked open question in `orbistoun-cli questions`.
