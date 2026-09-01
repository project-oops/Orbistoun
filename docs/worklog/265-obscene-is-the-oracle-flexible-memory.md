# 2026-09-01 - obSCEne is the oracle; flexible-memory fixed byte-exact against hardware


User's steer: obSCEne passes on hardware, so its crash under orbistoun is an orbistoun bug - and it's the
ideal oracle (555 checks with recorded hardware answers). Two parts.

Stale eboot: `titles/obscene/eboot.bin` was yesterday's build and crashed early - its backward ELF-magic
base scan ran off the mapping because that build didn't import `sceKernelVirtualQuery`, disabling its own
guard (not chiefly orbistoun's fault; a vendor module's header is legitimately not resident at the base).
selfish builds again (the user deployed obSCEne today), so rebuilt `make module` fresh and installed it as
the title. It now runs 338 checks under orbistoun.

Flexible memory, measured exactly by the oracle vs `console-klog.01092026.txt`: `flexible-available` was
answering `~0x1_3f01_0000` (the direct pool) where hardware says `0x1b40_0000`, and `flexible-configured`
failed (unimplemented) where hardware says `0x1c00_0000`. Fixed: flexible is now a separate budget in
`direct` (configured `0x1c00_0000`, launch available `0x1b40_0000`, atomic mapped counter);
`sceKernelConfiguredFlexibleMemorySize` implemented; `available` reads the budget minus mapped;
`map`/`release` charge/credit it and `map` refuses past it. Both are the system default (no title overrides
the mem-param, D442) so they transfer. Re-ran obSCEne under orbistoun: both checks now match hardware
byte-exact (`available 0x1b400000`, `configured 0x1c000000` pass). Unit test pins the budget arithmetic;
kernel/mem tests pass, clippy clean, fmt-clean (D444).

Next wall: obSCEne faults at `image+0x4502ff` reading `0x600000801000`, one page past the guest stack -
a later probe walks off the stack. That's the next investigation.

