# D430 - Reentrant guest execution, and its first user: std::call_once


**measured** - 2026-09-01 (user-directed)

The subsystem the real titles were blocked on, built: calling a guest function *back* from inside an
HLE call and continuing. The primitive is `orbistoun_abi::enter::enter_guest_with_three_arguments`
(the two-argument entry, with `rdx` supplied rather than clobbered - what an `InitOnce`-shaped
callback needs) plus `thread::call_guest`, which reserves a fresh guest stack in its own arena
(`0x6800…`, clear of the thread stacks and the mapping arena), enters the callback there on the
current thread, and frees the stack when it returns. A fresh stack because the caller is mid-handler
on the guest thread's own stack; the same thread because a callback reads thread-local state. Tested
in isolation - three arguments in, their sum back - before any title relied on it.

Its first user is `std::_Execute_once`, the engine under `std::call_once`. Stubbed, it never ran the
initialiser, so every `static` a C++ program guards with `call_once` - most of them - stayed null and
the guest read a null out of it. Now the initialiser runs, on the fresh stack, once: the flag's first
word is this implementation's to define (`0` unrun, `1` done; a `once_flag` constructs to zero, so an
unrun flag needs no cooperation), and only a callback that reports success in the console's `InitOnce`
convention marks it done. Declared in `orbistoun-libc` (a title imports it there) and implemented in
`orbistoun-kernel` (where the reentrant call and thread registry live) - the D367 split, listed in the
kernel's declared-elsewhere exceptions. Not yet serialised across threads; nothing measured races it.

Measured on PPSA25872: 14→16 imports, verdict MIXED (more of the interface, along a new path) - the
call_once initialisers ran and it advanced past the `read of 0x5` they were stuck behind.

Also this pass: `sceKernelReserveVirtualRange` reserves against the mapping arena with a
conflict-retry (a base the address counter hands back can already be held; the reservation is what
knows, so a conflict takes the next base), writing the base through the `void **`. It is correct HLE
and removes a placeholder, though the two titles past it hit a *shared* wall it does not fix.

**The shared `0xfffe0` wall, then cleared.** PPSA02664 and PPSA25872 both faulted `write to 0xfffe0`,
and a probe found the cause: they call `sceKernelReserveVirtualRange` with a *specific* hint,
`0x5000_0000_0000`, and orbistoun was ignoring it and reserving elsewhere - so the guest's own
allocator, which addresses its range from the hint, wrote where nothing was reserved. Honouring the
hint (falling back to the arena only on conflict) cleared it for both.

Past it, both hit `sceKernelVirtualQuery` - the guest walks its address space to place things, and it
was a placeholder answering a mapping that was not there. Implemented against the same `mappings()`
address space: it finds the region holding the queried address and writes its start and end (offsets
0 and 8, the two fields a guest bounds a mapping with; the rest of the struct has no lawful layout
here), or answers the console's not-found code. Both titles went FURTHER on it - PPSA25872 to 20
imports, PPSA02664 to 27 - and now reach `sceKernelMapDirectMemory` / `sceKernelAllocateMainDirectMemory`,
the next wall. So the reentrant primitive opened a run of memory-management walls, each of which the
next fix advances - the road the loop is for.

