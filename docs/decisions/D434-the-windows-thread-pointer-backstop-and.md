# D434 - The Windows thread-pointer backstop, and PPSA28061 past its TLS wall


**measured** - 2026-09-01 (user-directed, /loop)

The Windows half of D433, built and working: since Windows resets the guest `fs` base to zero on the
next context switch, a fault-handler backstop restores it on demand. `tls_backstop::remember` records
the thread pointer per thread when it is installed; the vectored fault handler, on an access violation,
calls `restore_if_reverted` - which re-installs the base and retries the instruction **only when the
base has actually reverted to zero**, leaving every genuine fault for the reporter. Sound because a zero
base sends every `fs:`-relative access into the unmapped ±2 GiB around zero (a signed 32-bit
displacement cannot reach the arenas at `0x4000…` and above), so such an access always faults here
rather than reading wrong data - there is no silent-corruption case to miss.

Measured: PPSA28061, the furthest title, went **FURTHER** - 56→60 imports, the `mov rax, fs:[0]` wall
gone. It now runs its texture loads and reaches its leaderboard/JSON init, printing
`sce::Json::Initializer::initialize failed: 0x7fff0001` and calling `abort()` - a graceful give-up, not
a fault. The `0x7fff0001` is the `Unimplemented` placeholder: `_sceUltMutexCreate` (libSceUlt, a named
mutex the JSON initialiser creates for thread-safety, arg1 = "ultmtx") answered the placeholder, the
initialiser read it as failure, and the leaderboard aborted. The same D125 shape the `_Mtx_*` family
already fixed, one library over. No regression: PPSA25872 and PPSA02664 unchanged. The backstop costs
nothing on a base-preserving host (Linux): `restore_if_reverted` simply never finds the base zero.

