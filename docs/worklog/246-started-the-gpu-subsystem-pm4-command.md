# 2026-09-01 - started the GPU subsystem: PM4 command builders (D427)


165-gnm was the tractable end of the GPU work: the command *builders*, which write PM4 into a guest
buffer and touch no GPU. orbistoun had the walker (read side) but no writer and no implementations()
for libSceGnmDriver. Added packet::build (the write side, same header format, round-trip tested),
implemented sceGnmDispatchDirect (documented 5-dword DISPATCH_DIRECT packet -> 0) and
sceGnmDispatchInitDefaultHardwareState (reserve 0x100, fill with no-op packets -> 0x100), wired
orbistoun_gpu::implementations() into service::symbols. Both checks pass (were partial). dispatch-init
0x100 matches hardware; dispatch-direct 0x5 vs hardware 0x6 - the console writes one dword more than
the documented packet, left unmodelled rather than guessed, inside a pass. gpu tests + clippy clean.

Tally now 516 pass / 8 partial / 4 fail / 15 skip. The subsystem is started - builder + walker + the
first two guest-facing calls; the submit -> walk -> Vulkan-translate path is the road from here (the
submit/flip calls stay declared-only, deliberately, until translation exists to back them).

