# 2026-08-30 - Word zero is getpid, and the payload's own arithmetic lands on real functions


obSCEne had already measured the payload ABI (its D208/D209): elfldr hands a payload getpid's
address as `payload_args[0]` and resolves nothing else, and the CRT computes everything from it -
`libkernel_base = args[0] - 0x5b0`, every export at `base + vaddr`, the vaddrs read by selfish
off the real 12.40 file. This project had been putting the dlsym resolver at word zero, which the
CRT never expects, so `base` came out garbage and every payload bailed to the same error exit.

Fixed: the firmware skeleton now lays libkernel out inside itself - each measured export's own
thunk copied to `base + vaddr` (the thunks are position-independent, so a copy works) - and hands
getpid's address there as word zero. The region became writable-and-executable, the one such in
the project, because a firmware image is where code and data share a space.

**It works as far as it should and no further.** The CRT read word zero as getpid, computed the
base, and called getpid through the region - syscall 20 fired, dispatched by this project's own
getpid. So the tens-of-megabytes offset arithmetic is not reaching a kernel; it is reaching
libkernel exports by vaddr, and now they are there.

It stops at the same error exit, because the next CRT step is the sandbox escape - `rwpipe`,
`rwpair`, `kpipe_addr`, `kdata_base_addr` and 12.40 kernel offsets, which D208 already flagged as
the not-brute-forceable part. Those struct fields are still markers, so the escape fails. Word
zero was necessary and is not sufficient; the escape is the next layer.

The vaddr table is two entries - the two `boot.c` cited. The full 1,867 selfish read are being
exported as measured data; when they land this becomes a load and the CRT reaches more before the
escape.

