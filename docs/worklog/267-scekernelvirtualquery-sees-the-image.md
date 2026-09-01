# 2026-09-01 - sceKernelVirtualQuery sees the image and stack (obSCEne vq-text/vq-stack pass)


Next two obSCEne divergences (D445): `virtual-query-text` and `virtual-query-stack` both refused
(`0x80020002`). `sceKernelVirtualQuery` only searched the runtime map (`mappings`), which never holds the
loaded image, the stack or the TLS block - they live in the loader's/worker's own address spaces. Added a
`note_region` registry (the worker notes the image span; stack/modules were already noted for
`sceKernelIsStack`/`GetModuleList`) and a `region_containing` that consults the runtime map, noted regions,
this-thread stack and the main stack span; both `virtual_query` and the stack test go through it. Oracle
confirms: `virtual-query-text` → pass `0xa2b000`, `virtual-query-stack` → pass `0x800000` (both matching a
console mapping); obSCEne failures 7→5 distinct, no new ones. Unit test pins the registry. Kernel/worker/mem
tests pass, clippy clean, fmt-clean (D446).

Noted for a later turn: `virtual-query-unmapped` is `partial` (pre-existing) - it queries `0x720000240000`,
which orbistoun's arena maps but the console refuses (`0x8002000d`); a separate arena-placement divergence.
Remaining fails: `110-modules` (one-module gap), `135-sysctl/osrelease` + `137-kernelcall/system-version`
(refused; both measured on hardware, answerable from the configured machine), `900-surface/control`
(resolver reports a non-existent symbol present).

