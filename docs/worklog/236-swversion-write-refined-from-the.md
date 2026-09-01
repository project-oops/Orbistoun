# 2026-08-31 - SwVersion write refined from the hardware dump


While adding obSCEne export candidates, checked what the last hardware run already dumped and found
the SwVersion layout was there: `130-layout/system-software-version` shows the call writing the
version string at offset 8 and the version integer at 0x24, and leaving offset 0 (the caller's
size) untouched - extent 40, changed 32. The D416 implementation wrote a full 0x28-byte struct
including a size word at offset 0. Refined it to write only 8..0x28, so orbistoun's dump now
reproduces the hardware structure exactly (offset 0-7 stays poison, changed 32). A small fidelity
fix; the check passed either way, but a guest reading its own size field back now sees what it set,
as on hardware. Kernel tests + clippy clean.

