# 2026-09-01 - sceKernelVirtualQuery + the C-runtime threading family (D431)


(kernel) `sceKernelVirtualQuery` implemented against `mappings()`: finds the region holding the
queried address, writes its start/end (offsets 0/8), or answers not-found. Cleared the wall past
`sceKernelReserveVirtualRange` for both real titles - PPSA25872 20 imports, PPSA02664 27, both FURTHER.

(kernel/libc) The C-runtime threading family - `_Mtx_*` (init/lock/unlock/trylock/destroy), `_Cnd_*`
(init/wait/timedwait/signal/broadcast/destroy), `_Xtime_get_ticks`, `_Thrd_sleep` - the `std` layer
above `_Execute_once`. Each maps onto the honest `sync` primitives (real mutual exclusion), declared
in libc, implemented in kernel, in the declared-elsewhere exceptions. Removed `_Throw_C_error` and all
13 from the unimplemented list at once: stubbed they answered the `0x7fff0001` placeholder, which the
standard library threw (D125 shape). SURPRISE worth recording: PPSA25872 went 20→19 imports (BACK) as
this landed - the throw was a side path that called imports before dying, so removing it lowered the
count while the fault PC stayed put (`image+0x1668a51`). A correct fix that moves no wall and lowers the
metric; recorded as infrastructure, not progress (principle 3).

Next wall is instruction-level and the SELF/eboot wrapper hides the bytes (ELF `p_offset` does not
locate loaded code; only the loader's wrapper decode does). Recorded a `dump`/`disasm` verb as the
honest next step rather than guessing. Kernel tests green (74+33+33); orbistoun-kernel clippy-clean
(the orbistoun-fs escape/socket clippy debt is pre-existing, not this session's).

