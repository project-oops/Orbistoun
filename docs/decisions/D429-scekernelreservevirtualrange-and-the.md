# D429 - sceKernelReserveVirtualRange, and the boundary the real titles reach


**measured** - 2026-09-01

Implemented `sceKernelReserveVirtualRange`: it reserves a fresh range from orbistoun's own high
arena (`next_mapping_base`) and writes the base back through the `void **` the guest passed, instead
of the stub-everything placeholder a guest stored as a base and then wrote through into a fault.
Honouring the guest's low address *hint* first regressed (its own allocator wrote just outside the
range orbistoun reserved there); a fresh base orbistoun owns is neutral and correct. Kept because it
is called by titles and removes a placeholder-as-data (D125), even though it does not by itself
advance the titles measured - their walls are deeper.

And those walls are the honest boundary of the crunch. Recorded so a future session builds rather
than re-discovers:

- **A reserve/commit memory model.** `orbistoun-mem::reserve` is always `MEM_RESERVE | MEM_COMMIT`
  and treats a second reserve of a range as a conflict. A console reserves address space and *then*
  maps into it; modelling that needs a reserve-only path plus a commit-into-reserved path, and the
  map calls taught to use them. PPSA02664 faults `write to 0xfffe0`, a low fixed address independent
  of the reserve fix - unclear without deeper RE, and possibly this same gap.
- **Reentrant guest execution.** `std::call_once` (`_Execute_once`) must call a guest function
  pointer from inside an HLE call and return - nested stack-switching over the thunk that
  `enter_guest_with_argument` does not do today. PPSA25872's C++ runtime needs it; done hastily it
  crashes and lies, so it is a design, not a sweep.
- **Libraries.** PPSA28061's null-read is libSceJson2/libSceNpCppWebApi objects left null by stubbed
  initialisers; its other gaps are libSceUlt (user threads) and libSceAgc/libSceAgcDriver (the GPU
  API). The simple Agc getters return values no lawful source here documents - guessing them is the
  invention principle 3 forbids.

None is blocked on data or on the (non-existent) parallel session. Each is a multi-step subsystem
build. sysmodule (D428) and this were the last one-call fixes the current data supports; the road on
is picking one of the above and building it out with the same discipline, not crunching it.

