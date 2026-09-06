# 2026-09-01 - (/loop) Overnight crunch: putchar and std::random_device

The user asked for an overnight, oracle-free crunch of everything that does not need data.
Triaged the remaining corpus walls first: they all trace to *unimplemented* functions answering
placeholders, not logic bugs like the map one (D460), so the crunch is "implement the ones with a
standard/citable contract, skip the hardware-gated ones, name the unnamed hashes". Aggregated the
exact demand from the corpus run reports.

Implemented two, both from PPSA21564, both fully oracle-free:

- **`putchar`** - one byte to the output stream, returns it. Routed to stderr like `puts`/`printf`
  (D344). Published contract (ISO C / POSIX), `known_by = published`.
- **`_ZSt14_Random_devicev`** (`std::_Random_device()`) - the entropy primitive behind
  `std::random_device`. Implemented as a deterministic `splitmix64` sequence for reproducibility;
  see D461 for why real entropy would be wrong here. `known_by = assumed` (the symbol-to-contract
  mapping is inferred).

`cargo clippy`/`fmt` clean on libc; libc tests pass (87); the knowledge audit passes (22) after
two corrections worth noting for next time: the `returns` field is a **typed enum**
(pointer/handle/count/status), not free text - omit it for a scalar-returning function as `puts`
does; and an **`assumed` entry must carry no `cites`** (assumed means nothing confirms it), so the
splitmix64/C++ references live in the source doc comment and the reasoning in `assumptions`.

Honest outcome: neither moved PPSA21564's wall - it still stops at the `int 0x41` assert
(`image+0x11ccd`) after exactly 500257 calls, unchanged. That number being *identical* across two
runs is the useful signal: the deterministic `random_device` kept the run reproducible, where a
real-entropy one would have made the count drift. So this was stub reduction (no more placeholder
poison from these two), not a crack. The `int 0x41` root is elsewhere - the flagged unimplemented
`sceKernelGetGPI` is hardware-gated and the likelier cause.

Next oracle-free steps, in yield order: a **naming pass** on the 53 unnamed hashes (naming
`PS5Util::0xf948d02a4f9f5ace`, 23413 calls, could unblock PPSA25872), then the remaining standard
functions (`vsprintf_s`, and the advisory `sceKernelSetVirtualRangeName` / `scePthreadSetaffinity`
whose effects are unobservable to the guest), then deeper wall analysis for logic bugs.
