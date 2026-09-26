# orbistoun-kernel

libkernel: memory syscalls, threads and synchronisation. Third in the dependency spine,
after the container parser and the address space; no subsystem above it is reached until a
guest has allocated memory and started threads through this layer.

It covers the direct-memory syscalls (`direct`), thread creation and join, mutexes and their
attributes, and semaphores, and declares the rest of the pthread surface. It builds on
[orbistoun-mem](../orbistoun-mem/) and [orbistoun-thunk](../orbistoun-thunk/);
[orbistoun-posix](../orbistoun-posix/) serves the POSIX spellings of its calls.

## Rules

- **FreeBSD is the reference.** The target kernel is FreeBSD-derived and much of libkernel is
  POSIX under vendor naming, so most functions have a documented, citable analogue. Name it in
  a comment when implementing one.
- **Guest threads are real host threads.** A green-threaded or pooled implementation cannot
  work: guest code reads thread-local storage directly and blocks in its own primitives.
