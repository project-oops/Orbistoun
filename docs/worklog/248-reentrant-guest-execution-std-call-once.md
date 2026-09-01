# 2026-09-01 - reentrant guest execution + std::call_once (D430)


Built the subsystem the titles needed: enter_guest_with_three_arguments (abi) + thread::call_guest
(kernel) - call a guest function back from inside a handler, on a fresh stack on the current thread,
and continue. Tested in isolation (three args in, sum out). First user: std::_Execute_once, declared
in libc, implemented in kernel. PPSA25872 got FURTHER (14->16 imports, MIXED) - call_once initialisers
now run, past the read-of-0x5 they were stuck behind. Also implemented sceKernelReserveVirtualRange
(conflict-retry reservation, base written back).

Both PPSA02664 and PPSA25872 now hit a SHARED wall: write to 0xfffe0 (0x100000 - 0x20), with 0x100000
in registers independent of ReserveVirtualRange. A fixed 0x100000 structure both runtimes expect;
needs the guest's instructions to pin, not more HLE. Highest-leverage remaining wall because shared.
Obscene 516 pass, no regression; kernel/abi tests green; my code clippy-clean (the fs escape/socket
clippy errors are pre-existing orbistoun debt, not this session's - and not a parallel session's,
which does not exist).

