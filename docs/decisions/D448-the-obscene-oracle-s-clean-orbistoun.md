# D448 - The obSCEne oracle's clean orbistoun bugs are mined out; the rest are by design


**measured** - 2026-09-01 (user-directed /loop; obSCEne oracle)

obSCEne running its whole suite under orbistoun (D444) drove five real fixes in as many turns - the C++
allocator wall (D443), the stack read-ahead guard (D445), `sceKernelVirtualQuery` seeing the image and
stack (D446), and `sysctlbyname` answering `kern.osrelease` (D447), plus flexible memory (D444). Its failure
set fell from the crash-at-check-forty state to four. Those four are **not clean bugs**, and this records
why, so a later turn does not mistake a deliberate design difference for something to fix by gaming a probe.

- **`900-surface/control`** - orbistoun reports a symbol that cannot exist as present. This is the
  loud-stub-everything model (D392): every import resolves to a stub so an unimplemented call is *reported*
  rather than a jump into a zeroed slot, which is exactly what took PPSA02664 from a crash to 1541 calls.
  The mechanism to fix it exists (`ImportResolver.refuse`) and the correct, hardware-matching form is
  *narrow*: refuse only imports that are **weak and name no symbol orbistoun knows**, since a console leaves
  a weak undefined symbol null while a strong one fails to load - so obSCEne's weak, invented
  `obs_census_control_absent` would go null (absent) while every real, known import keeps its stub. It is
  deferred rather than done here on purpose: it needs the ELF reader to expose the symbol *binding* (only
  the type nibble of `st_info` is decoded today) and the service to compute the refuse set from
  weak-and-unnamed, and it **re-baselines every measurement** taken under `refuse = None` (D392's own
  caution). That is a measured, deliberate step, not a loop-tick flip of the most foundational path.

- **`110-modules/info-size` and `/names`** - `sceKernelGetModuleInfo` is unimplemented, so no size is
  accepted and no module is described. orbistoun does place exactly one module and `sceKernelGetModuleList`
  reports it (deliberately not a plausible-looking list of libraries it did not load); describing it needs
  the `SceKernelModuleInfo` layout, which obSCEne's own check says it "does not fully know". Inventing that
  layout is the D008 error. Gated on a citable layout, like the mem-param was (D442).

- **`137-kernelcall/system-version`** - skips on hardware, where obSCEne could not resolve `getpid` to build
  its syscall gadget (`0x80020003`); orbistoun *does* resolve `getpid`, builds the gadget, reaches the raw
  syscall and answers `-78` (`ENOSYS`). The divergence is the same resolve-everything model as `900`, one
  layer down, plus honest refusal of an unimplemented raw syscall. Not a knob to answer.

So the oracle has done its job for now: the fixable divergences are fixed and confirmed byte-exact against
the console, and the remainder are design choices (`900`, `137`) or blocked on a citable struct layout
(`110`). The next frontier is not another obSCEne check but a **real title** - PPSA02664 runs 1541 calls and
walls in `libc::_Getpctype`, the ctype-table accessor, whose table FreeBSD documents (the lawful oracle,
principle-1). That is where the clean work is now.

