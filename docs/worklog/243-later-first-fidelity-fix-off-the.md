# 2026-08-31 (later) - first fidelity fix off the complete run: GetModuleInfo refusal code


With the full suite finally running, the 4 fails were legible. sceKernelGetModuleInfo refused with a
bare -1 (0xffffffff); obSCEne's 110-modules records the console refusing it with 0x8002_0016
(INVALID) across both hardware runs. Aligned the refusal to the measured vendor code - the same
assumption-over-measurement slip as the software version (D420). 110-modules/info-size and /names now
fail byte-for-byte as hardware does. Kernel tests (74) + clippy clean. The remaining 110-modules
differences are load-count (orbistoun runs one module, hardware has ~32) - expected, not a gap.

