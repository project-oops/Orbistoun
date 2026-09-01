# 2026-09-01 - (/loop) scePthreadGetthreadid; PPSA21564 now runs at 0% stubs

With PPSA21564 booting (D451), its hot loop called `scePthreadGetthreadid` 22.5k times against the
placeholder, so every thread reported one shared id. Implemented it in `orbistoun-kernel` as the calling
thread's registry handle (`thread::adopt`) - the same unique-per-thread value `scePthreadSelf` answers, and
a safe zeroed-block address if the guest dereferences it (the D151 reasoning). Recorded D452.

Result: PPSA21564 runs at **0% stubs** - 499082 of 499087 calls implemented, the five remaining being
singletons (`_ZSt14_Random_devicev`, `_init_env`, `sceKernelGetGPI`, `pthread_key_create`). Runs to the call
budget with no fault, repeatably. Getting further now needs the budget raised or the loop's wait
(video/input) wired, not another missing import.

`fmt`/`clippy`/tests/knowledge audit pass for the additions. The kernel crate carries pre-existing fmt drift
from earlier-session work (benign line-wrapping in tests/comments, deliberately left un-reformatted); none of
it is in this iteration's additions, which are clean. Additive change (one new symbol); other titles
unaffected.
