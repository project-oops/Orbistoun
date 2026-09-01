# D407 - Word zero is getpid, and libkernel is laid out by measured vaddr


**guest-observed** - 2026-08-30

The payloads were handed the wrong thing at `payload_args[0]`. This project put the *dlsym
resolver* there; obSCEne measured that elfldr puts **getpid**, and resolves nothing else - the
payload's CRT computes `libkernel_base = args[0] - 0x5b0` and reaches every other export at
`base + vaddr` (obSCEne D208, D209, both `hardware`). getpid's vaddr, `0x5b0`, and the others
were read by selfish off the real `libkernel_sys.sprx` - the platform's own file as oracle, no
SDK, no guess.

### What was built

When a run presents a firmware, the firmware skeleton now lays libkernel out inside itself: each
export this project has a measured vaddr for gets that function's own thunk copied to
`LIBKERNEL_BASE + vaddr`, and `getpid`'s address there is handed as word zero. The thunks copy
cleanly because they are position-independent - absolute call targets, a self-relative internal
jump - so one works wherever it is placed. The region is mapped writable-and-executable, the one
such region in the project, because a firmware image is where code and mutable data share a
space; the exception is stated where it is made.

A firmware-absent run keeps the old resolver-in-word-zero behaviour untouched, so nothing that
was working changes.

### What it proved, and what it did not

**Proved:** the scheme works. A payload's CRT read word zero as getpid, computed the base, and
**called getpid through the laid-out region** - syscall 20 fired, dispatched by this project's
own implementation. The firmware-offset arithmetic the payloads do is not reaching into a kernel;
it is reaching libkernel exports by their vaddrs, and now it lands on real functions.

**Did not:** move the wall. The CRT still stops at the same error exit (`image+0x2acf8`), because
its *next* step is the sandbox escape - the one that uses the struct's other fields (`rwpipe`,
`rwpair`, `kpipe_addr`, `kdata_base_addr`) and 12.40 kernel offsets, which obSCEne's D208 already
named as the part that is not brute-forceable. Those fields are still markers here, so the escape
fails and the CRT bails exactly as before.

So this is a necessary fix that is not yet a sufficient one: the ABI at word zero is now correct
and the layout mechanism is real, and the escape is the next and harder layer. It is recorded
`guest-observed` because getpid dispatching through the region is observed, while the vaddr table
is a two-entry stub of a measured file that is still being exported.

### The full table exposed a packing collision, now fixed

With all 1,867 vaddrs laid out, the real spacing showed: libkernel functions sit **0x20 bytes
apart** - getpid `0x5b0`, mount `0x5d0`, unmount `0x5f0`. getpid's 64-byte landing-zone thunk ran
straight over its neighbours; its own dispatch landed at `+0x20`, exactly where mount's stub goes,
so the two corrupted each other and a call to getpid ended up in mount's stub. The named
work-list handler reported it as "unimplemented mount", which was the symptom, not the call.

getpid now gets a **23-byte compact slot** that fits the gap: `mov eax, 20` (SYS_getpid) padded to
offset 10, then a jump to the syscall gadget, so a plain call issues getpid and getpid+10 reaches
the gadget with the caller's number - the +10 convention preserved without the 64-byte body. Every
other export is a 13-byte trampoline, which fits 0x20-byte spacing. With getpid whole, a payload's
early getpid probes succeed (six syscalls became eight) and it advances to the sandbox escape,
which is the next and separately-hard layer (D208).

The unimplemented-export handler now names the export from the vaddr its stub passes, so a payload
reaching a libkernel function this project has a vaddr for but no implementation of prints one
work-list line rather than a bare "an export".

### The table is meant to grow

`LIBKERNEL_EXPORTS` holds two entries - the two `boot.c` already cited. selfish read all 1,867;
when that table is exported as measured data, this becomes a load rather than a constant, and the
CRT will reach more of what it needs before the escape.

