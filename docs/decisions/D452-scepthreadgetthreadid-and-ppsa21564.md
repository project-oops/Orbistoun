# D452 - scePthreadGetthreadid answers the thread's handle, and PPSA21564 reaches 0% stubs


**measured** - 2026-09-01 (user-directed /loop)

With the mspace family (D451) letting PPSA21564 boot, its hot loop asked `scePthreadGetthreadid`
22.5k times, and unimplemented it answered the placeholder - so every thread reported one shared id, which
a guest keying anything on thread identity reads as a single thread.

**The representation choice.** `scePthreadGetthreadid` (FreeBSD `pthread_getthreadid_np`) answers a
per-thread integer id. It is implemented in `orbistoun-kernel` as `thread::adopt("main")` - the calling
thread's **registry handle**, the same value `scePthreadSelf` answers. Two reasons this is right rather than
lazy:

- The handle is already a `u64` unique to the thread and stable for its life (`NO_THREAD` is 0), so it *is*
  a valid unique id. Adopting the caller gives the process's first thread - which runs guest code without
  ever being created - a real id rather than zero, exactly as `scePthreadSelf` does.
- Returning the handle rather than a small integer follows the deliberate D151 choice: a handle is the
  address of a real zeroed block, so a guest that treats the id as something to dereference lands on safe
  memory, where a small integer reproduced a `read of 0x5`-class fault.

The cost is that `scePthreadGetthreadid` and `scePthreadSelf` return the same value; a guest that expects the
`ScePthread`-handle and thread-id namespaces to differ is not modelled. Recorded `assumed`, since the id
namespace and width are inferred from the name, not verified against the target library.

**Milestone.** PPSA21564 now runs at **0% stubs** - 499082 of 499087 calls answered by an implementation,
the five remaining being singletons (`_ZSt14_Random_devicev`, `_init_env`, `sceKernelGetGPI`,
`pthread_key_create`, called once or twice each). It runs to the call budget with no fault, repeatably. A
title that faulted at 131 calls a session ago now boots and runs its main loop with every hot-path import
satisfied; getting *further* now needs the budget raised or whatever the loop waits on (video/input) wired,
not another missing function.

`fmt`/`clippy`/`cargo test`/the knowledge audit pass for the additions (the kernel crate carries unrelated
pre-existing fmt drift from earlier-session work, deliberately left un-reformatted; none of it is in these
additions). The change is additive - one new symbol - so other titles are unaffected.
