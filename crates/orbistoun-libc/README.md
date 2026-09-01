# orbistoun-libc

The C library, as the guest calls it. The largest implemented surface in the project.

**Models:** 44 functions - memory (`malloc`, `free`, `calloc`, `realloc`, `memalign`),
strings (`strlen`, `strcmp`, `memcpy`, `memset`, ..), formatted output (`printf`,
`snprintf_s`), the C++ ABI (`operator new`/`delete`, guard variables, `__cxa_atexit`), and
process exit.

**Deliberately fakes:** nothing it implements. What is declared and unimplemented answers
the stub policy and says so.

**Design note.** Chosen by measurement, and the measurement was emphatic: every title that
*faulted* rather than spun was faulting because of this. One called `memset` three hundred
times then wrote to `0x7fff0119`; another called `strlen` and `memcpy` two hundred and
forty-eight times each then read `0x5`. They were being told "not implemented" and
carrying on with the answer (D123).

`strlen` returning an error code instead of a length is not a missing feature - it is a
guest that now believes every string is fourteen bytes long. The damage surfaces somewhere
unrelated, which is why those looked like three separate mysteries.

**This is the easiest correct code in the project.** Everything else here guesses at
undocumented semantics. ISO C and POSIX say precisely what these do, the target library is
FreeBSD-derived, and both are citable. No oracle problem, so no excuse for any of it being
subtly off.

**Guest pointers are host pointers.** The address space is identity-mapped, so a pointer
the guest hands over is dereferenced directly - which is why every one of these is `unsafe`
and why the guest is trusted about its own arguments, exactly as the real library trusts
them. A guest that passes a bad pointer faults here precisely as it would have faulted
there, and the report names the address.

**Status:** 44 functions implemented. Two titles print their own diagnostics through
`printf`, which is how four other functions came to be named.
