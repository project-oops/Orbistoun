# 2026-09-01 - Shared-allocator diagnosis: wrong titles, real gap is `Configured` (D442)


Investigating the flexible-memory "shared allocator" work. Import-surveyed all four resident titles
(`orbistoun-cli imports`): PPSA02664/25872/03416 import **no** flexible-memory function - only
`sceKernelGetDirectMemorySize`, which orbistoun answers correctly (0x1_4000_0000). So the standing theory
that their C++ allocators size off flexible-available is false; there is nothing to fix for them. Only
PPSA21564 uses flexible memory, and it imports `sceKernelConfiguredFlexibleMemorySize` (the configured
total, distinct from available) - which orbistoun does **not** implement. That is the real gap.

Did not implement it: the honest value for a game is that game's declared budget, read from its
`PT_SCE_PROCPARAM` mem-param - a binary-format read that belongs in **SELFish** (`selfish-elf`), from a
citable source, not invented in orbistoun (principle 3). SELFish is mid-rescaffold by an active session
right now (empty root `Cargo.toml`, new `selfish-cli`, README touched the same hour) so it neither builds
nor is mine to edit; and PPSA21564's 1555 unresolved imports mean it is unproven it even reaches the call
(principle 6). Recorded the corrected diagnosis (D442) and paused for direction on the SELFish dependency.
`orbistoun-cli` builds clean; no code change this unit.

