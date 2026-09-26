# orbistoun-libc

The C library, as the guest calls it.

It implements memory allocation (`malloc`, `free`, `calloc`, `realloc`, `memalign`), strings
(`strlen`, `strcmp`, `memcpy`, `memset`, ...), formatted output and scanning (`printf`, `snprintf_s`),
character classes, maths, clocks, atomics, streams, the C++ ABI (`operator new`/`delete`, guard variables, `__cxa_atexit`) and
process exit. What is declared and not implemented answers the stub policy and says so. It
builds on [orbistoun-fs](../orbistoun-fs/) for streams and
[orbistoun-thunk](../orbistoun-thunk/) for dispatch.

## A C library cannot be stubbed

A guest told "not implemented" by `strlen` carries on with the answer: it believes every
string is as long as the error code, and the damage surfaces somewhere unrelated, far from
the call. So a C library function a guest calls is implemented, not left to stub policy.

ISO C and POSIX say precisely what these functions do, and the target library is
FreeBSD-derived, so both are citable. There is no oracle problem here and no reason for any
of it to be subtly off.

## Guest pointers

The address space is identity-mapped, so a pointer the guest hands over is dereferenced
directly. Every such function is `unsafe`, and the guest is trusted about its own arguments
exactly as the real library trusts them. A guest that passes a bad pointer faults here as it
would have faulted there, and the run report names the address.
