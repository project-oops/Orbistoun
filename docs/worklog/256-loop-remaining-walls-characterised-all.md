# 2026-09-01 (/loop) - remaining walls characterised; all need obscene/external data (D436)


Disassembled the local titles' walls via a temp code-dump (removed after). PPSA25872 image+0x7b5890 = a
container allocator's max-size check returning 0 (upstream size too large). PPSA02664 AND PPSA03416 share
image+0xafcc08 = a C++ virtual allocate() returning 0 → vmovdqu [r12=0] null write. Both are the guest's
own allocator answering null after the VirtualQuery→AllocateMainDirectMemory→MapDirectMemory sequence -
every call implemented, so the fix is exact memory-subsystem semantics (VirtualQuery struct fields, map
shape/sizes), which is what obscene measures. PPSA28061 online blocker un-nameable locally; PPSA04263 is
the memory-map-shape question again. Conclusion: every remaining wall needs obscene hardware data or an
external symbol source - the loop's stop condition. Build clean, worker clippy-clean.

