# 521. The retail frontier is characterized; what remains is deep, not loop-tick work

**2026-09-12** - end of a long "test titles / iterate the wall" session

Every runnable title has been driven to its wall and the wall named. The tractable init-call wins are
taken; what remains needs deliberate builds or obSCEne data, not the loop's tick-by-tick cadence.

## Runnable corpus (titles/ holds exactly these)

Six retail titles + obSCEne's two probes. The homebrew/tool entries in `compat/` (ftpsrv, ps5debug,
websrv, ...) are records from another machine's run; their eboots are not present here, so they cannot
be tested from this tree.

## Where each retail title stands

- **Earthion (PPSA28061)**: mapper must-succeed gate; the engine (`game.bin`) needs a filled
  `sceKernelMapperGetParam` struct - Phase-6 AGC context (D677, worklog 516). obSCEne a2f9 pending.
- **PPSA02664 (PPSA02664) / Summer Sports (PPSA03416)**: common-dialog cleared (D678); both now reach the
  **AGC render-state object model** and fault on an unpopulated sub-pointer - Phase-6 GPU (worklog 517).
  obSCEne c7d1 pending.
- **ASTRO BOT (PPSA21564)**: a bundled-module null-deref, 500k calls in - the same GPU object-model
  layer (worklog 518).
- **GTA V (PPSA04263)**: no allocator bug - it demands ~8.4 GiB of direct memory (838 MiB + 3.06 GiB +
  4.51 GiB, no frees) against a hardware-correct 5 GiB pool. A per-title budget gap, needs a measured
  retail-game budget, not a code fix (worklog 520).
- **Terminator (PPSA25872)**: common-dialog + app-content init cleared (D678, D680), but its wall is a
  **cross-thread async-error assert**: `call [rbp-0x90]; mov rax,[rbp-0x88]; test [rax+0x30],0x10; je;
  int 0x41` - the main thread traps on an error flag a worker set, with another thread in
  `sceKernelSyncOnAddressWait` at the moment of the trap. Not a missing init function (mis-diagnosed as
  APR, then as app-content); the root is a failed async operation in a worker, likely the same render
  layer reached from a different thread. Deep.

## The shape of what's left

Four of five retail titles bottom out at the **Phase-6 AGC->Vulkan engine** (render-state / draw object
model), which is unbuilt and gated on obSCEne's c7d1/a2f9 object-model measurements. GTA V is a
memory-budget question. Terminator is a cross-thread async assert whose root is likely the same GPU
layer. None is a single-function implementation.

So the loop's productive "iterate the wall" phase is complete: the wins that were a return value or an
init sequence are done, and the tree is green with them. The next moves are deliberate - build Phase 6,
or wait on the obSCEne object-model data (a2f9/c7d1) that Phase 6 needs - rather than more per-call
implementations, which the walls are no longer made of.
