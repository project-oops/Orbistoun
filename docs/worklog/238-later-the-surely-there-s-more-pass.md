# 2026-08-31 (later) - the "surely there's more" pass found a wrong firmware value (D420)


Pushed to keep going rather than declare done, and it paid off. Ran a homebrew payload (ftpsrv) to
see why the corpus records them at 0x1/0-imports: they are freestanding elfldr payloads with no
dynamic imports, faulting `instruction fetch from 0x1` because they were entered with argc=1 where
they expect the payload_args handoff block. Making them run needs the handoff/gadget machinery in
orbistoun-abi (D365-D384), which is the parallel session's active area - left alone.

The obscene conformance re-diff turned out flaky to capture (the guest's report goes to a socket fd
or stderr nondeterministically; socket.rs is the parallel workstream's), so I mined the *static*
module hardware reports instead. Sysctl is already right and provenance-clean (hw.ncpu=16,
hw.pagesize=0x4000, kern.ostype="FreeBSD", kern.version/osrelease honestly refused rather than
inventing the firmware banner). But `sceKernelGetSystemSwVersion` was wrong: D416 wrote 12.40, and
the module dumps show 13.090.001 / 0x13090001 across three runs. The console has two version numbers
- system software 12.40 (syscall 649, kern.version banner, obSCEne sysinfo) and this call's 13.09 -
and D416 wrongly reconciled them. Corrected the constant, wrote the distinction into the doc-comment,
and pinned it with a test that asserts 13.09 and refutes 12.40. Kernel tests pass (72), clippy clean.

Surprise worth keeping: the bug was invisible from the payload report I was so pleased to absorb -
it has no struct dumps. It only showed up in the *module* reports, which were sitting on disk the
whole time. "No new hardware needed" was true; "nothing left to do" was not. Also absorbed the five
139-exports vaddr confirmations into libkernel-vaddrs.txt (7→12 confirmed, D419) in the same pass.

