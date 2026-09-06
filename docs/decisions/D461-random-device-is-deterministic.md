# D461 - std::random_device is deterministic here, on purpose


**assumed** - 2026-09-01 (user-directed /loop: overnight oracle-free crunch)

`_ZSt14_Random_devicev` (`std::_Random_device()`, the entropy primitive behind
`std::random_device`) was unimplemented and answered a placeholder the guest would have seeded
a generator with. Implementing it raises a question a real C++ library does not have to ask:
should it be **non-deterministic**, as the standard intends?

For this emulator, no. Every measurement it makes - `FURTHER`/`same`/`BACK`, a fault address, a
call budget - depends on two runs of one build behaving identically, which is the whole reason
the call budget replaced wall-clock timing (D181, D238). A physically-random `random_device`
would put non-determinism back into exactly the layer those decisions removed it from. A guest
reads this only to *seed* its own generator, so what it needs is a well-distributed 32-bit value,
not an unpredictable one; a fixed-seed sequence gives it that and keeps the run reproducible.

So it is a `splitmix64` sequence (Vigna, public domain) from a fixed seed, low 32 bits returned
(`result_type` is `unsigned int`), advanced once per call so successive reads differ. Verified
reproducible: PPSA21564 ran to exactly 500257 calls on two consecutive runs, the count unchanged
from before the implementation - a real-entropy version would have made that vary.

**Marked `assumed`**, because that this symbol is the entropy value-provider (rather than, say, a
constructor) is inferred from the mangled name and the guest's use, not confirmed against a
header for this exact runtime. It is a strict improvement on the placeholder regardless: harmless
if the guest ignores the value, correct if it seeds with it. The stance generalises - any future
entropy or clock primitive (`getrandom`, `gettimeofday`) should be deterministic-by-default or
carry a flag, never silently non-reproducible. It did **not** move PPSA21564's `int 0x41` wall,
which is caused elsewhere (the hardware-gated `sceKernelGetGPI`); this was stub reduction, not a
crack.
