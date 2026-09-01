# 2026-08-20 (evening) - The wall had a name, and it was C++


D124-D126. Named the biggest wall, implemented the C++ static-initialisation runtime,
replaced the hand-typed word list with a cited harvest, and found the systemic stub bug
underneath three unrelated faults.

### Surprises

- **The 53.5% import was `__cxa_atexit`.** Found by reasoning about the *shape* of the
  traffic - a libc function called more than all others combined is allocation or static
  initialisation, both of which are C++ ABI symbols no C word list produces - rather than
  by searching wider.
- **`__cxa_guard_acquire` was worse unimplemented than absent.** An error return is
  non-zero, which reads as "go ahead and initialise", and with `release` doing nothing the
  flag never set. Every function-local static reconstructing on every visit, forever.
- **An error code in a pointer register is a wild pointer.** Three titles faulting at three
  unrelated addresses shared one cause: functions that return handles were answering with
  error codes, which the guest dereferenced. Only visible from the *whole* call list - the
  worklist ranks by volume, and the function that ended the run was called twice.
- **My own harvest filter dropped the most important name in the corpus.** "Skip reserved
  names" excluded `__cxa_atexit`. FreeBSD already marks implementation detail with
  `FBSDprivate_*` blocks; a rule the source states beats one invented on top of it.
- **A citation named a temporary directory.** Useless to anyone re-deriving it. The
  revision leads now and the path is dropped.
- **`orbistoun-cli` could not build for an hour**, because it links the whole workspace and
  another session was mid-edit in the shader translator. The harvest now also exists as an
  example in a crate that depends on nothing but hashing.

### Outstanding

`read of 0x5` did not move after marking `scePthreadSelf` as pointer-returning, so either
the hypothesis is wrong or the wiring is not reaching it - unverified either way, and the
next thing to check. Thread pointer and previous-generation containers still untouched.


