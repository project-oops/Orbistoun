# 805. orbistoun serves the console's own syscall gadget at the fixed address a first-party payload falls back to, and gl1-cube runs its startup: nine system calls dispatch, it prints its own banner, and the next wall is the display device `/dev/dce`

**2026-09-22** — the wall worklog 804 left. gl1-cube (`GLCB00001`), now that it links, reaches
`Entered` and faults on its very first system call, executing at `0x8000004ea` — an address in no
region the run mapped. The fix serves that address, and the cube goes from dying on syscall one to
making nine, printing its own banner, and running to the time limit. Verdict **FURTHER**.

## Why a first-party payload calls a fixed absolute address for every syscall

The open-toolchain SDK routes every system call through a **gadget inside libkernel** — the raw
`syscall` instruction ten bytes into `getpid` — rather than executing `syscall` in the payload's own
text, to satisfy the platform's direct-syscall mitigation. When the runtime can resolve that gadget
by name (a handoff block with a working `dlsym`, the elfldr homebrew path) it uses `getpid + 0xa`.
When it **cannot** — a proper module gets no handoff block — it falls back to a hardcoded address:
libkernel loads at `0x8_0000_0000`, `getpid` sits at vaddr `0x4e0`, and the syscall instruction is
`getpid + 0xa`, so the runtime issues `callq *0x8000004ea` for every call. That address is baked
into the guest; nothing orbistoun hands it changes it.

orbistoun's firmware skeleton models a libkernel at its *own* base (`0xF0_0000_0000`) with the
gadget at `getpid + 10` there, and hands that address through the handoff block. A proper module
never receives the handoff block, falls back to the console's real `0x8000004ea`, and orbistoun had
**nothing mapped there** — so the guest's first `callq` faulted on an unmapped instruction fetch.
The retail titles never expose this: they reach the kernel through named libkernel imports resolved
at the library boundary, never through the freestanding gadget. gl1-cube, our own SDK's own source,
is the first guest to take the fallback path.

## The fix: map the console's gadget page and trampoline into this run's dispatcher

`CONSOLE_SYSCALL_GADGET_BASE = 0x8_0000_0000` is registered in `docs/ADDRESS_MAP.md` as the one base
this project did not choose — it is the console's, quoted. `orbistoun-firmware::present_console_gadget`
reserves one 16 KiB page there and writes a `mov r11, <gadget>; jmp r11` trampoline at
`+0x4ea` (`CONSOLE_SYSCALL_GADGET_VADDR`, `getpid + 0xa`) into this run's existing syscall gadget
(`orbistoun-abi::enter::syscall_gadget`). So the guest's `callq *0x8000004ea` jumps to the trampoline,
into the gadget that saves the syscall registers, calls `orbistoun_syscall_dispatch`, and returns —
a dispatch-and-return, exactly what the `syscall; jb +1; ret` at that address does on the console.

The worker sets it up **unconditionally**, not gated on a presented firmware: a proper module reaches
the gadget during its own startup without reaching the rest of the firmware image, so it is served
whether or not the elfldr firmware skeleton is stood up. A reservation failure is reported and not
fatal — the run proceeds exactly as before, the call faulting as it did.

## What the baseline then showed, which is the whole point of it

The run now reports, in order:

```text
[GLCB00001:GL-CUBE] display not ready        <- the guest's own banner, printed via the gadget
orbistoun: console syscall gadget served at 0x8000004ea
the guest made 9 syscalls, in this order:
    0  601  sceKernelDebugOutText  (0x7)
    1    4  write                  (0x1)
    2   20  getpid                 (0x0)
    3  601  sceKernelDebugOutText  (0x7)
    ... (601/4 repeating: the banner and status lines)
the guest asked for 1 path nothing here holds:  /dev/dce
progress  verdict FURTHER  executed code it could not reach before  (was 0x8000004ea)
```

Two things are now visible that were behind the syscall-one fault before:

- **`sceKernelDebugOutText` is call 601**, the kernel-log-output syscall the SDK spells `SYS_klog`.
  It is unimplemented, so the gadget answers it the kernel's `ENOSYS`; the guest's `gl_klog_line`
  ignores the return and carries on, which is why the banner still reaches orbistoun's own log.
- **The next wall is the display device.** The guest prints *display not ready* and asks to open
  `/dev/dce` — the display control device — which orbistoun's mount table does not hold. That is the
  rendering-path critical line: the cube is trying to reach the display to draw, and the device node
  it opens to do so is the next honest gap. This is precisely what backlog 037 argued the
  fully-owned baseline would surface — a real, buildable orbistoun gap on the road to the first pixel,
  which no retail title reached far enough to show.

## Gate state

`crates/orbistoun-firmware/src/lib.rs` gains `CONSOLE_SYSCALL_GADGET_BASE/VADDR/SIZE`, a
`present_console_gadget` / `console_gadget_address` / `is_console_gadget_present` trio on a page of
its own, and a read-back test. `crates/orbistoun-worker/src/lib.rs` wires it into the run path.
`docs/ADDRESS_MAP.md` registers the base and its exception to the tebibyte-spacing convention. The
placement writes into a page it reserved read-write-execute and bounds-checks first; no new `unsafe`
parses hostile input (principle 4). `orbistoun-firmware` tests 8/8, `orbistoun-service` address-map
sync 2/2, worker builds. `./bin/orbistoun check` green, worklog index regenerated, identity scan
clean. No commit.
