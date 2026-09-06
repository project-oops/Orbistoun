# 2026-09-02 - (/loop) The last clean functions; the oracle-free crunch is complete

Ran a full corpus sweep to confirm the night's cumulative state, then cleared the last standard
functions a sweep of the whole flagged list still showed as clean:

- **`wcsrchr`** (libc): last occurrence of a wide character, four-byte `wchar_t` as `wcslen` assumes;
  the terminator counts, so `wcsrchr(s, 0)` answers a pointer to it.
- **`scePthreadCondattrInit`** (kernel): allocates a condition-variable attribute object, the
  pointer-to-pointer shape of the whole attribute family (D272); an empty block a `CondInit` accepts.

clippy/fmt/tests/knowledge-audit clean. Neither moves a wall - surface reduction.

**Deliberately not implemented, and why**, because guessing them would be the honest-failure the whole
project is built against:
- `scePthreadSetaffinity`/`scePthreadGetaffinity` - the get's out-parameter width is unknown, so
  writing it would be a guess; and the pair is advisory (orbistoun does not pin guest threads).
- `malloc_stats_fast`, `_init_env`, `sceUserServiceGetGamePresets` - the struct or environment they
  fill is not known from any lawful source, and each was either reverted before or never safe to guess.

## The overnight crunch, in full

Across ~thirteen loop ticks, this is what "crunch everything that doesn't need data" reached. The
fixes that moved walls, each verified by a second observation (a call-count jump, not just a moved
fault):

| # | fix | effect |
|---|---|---|
| D460 | map commits into an existing reservation | +3 titles past the allocator |
| D463 | mapping arena moved off the thunk data-block base (a real `0x7200…` address collision) | PPSA04263 32 -> 10k calls |
| D464 | per-thread thread-local storage for spawned threads | PPSA04263 10k -> 333k calls |
| D465 | blocking `WaitEventFlag` on the event-flag condvar | killed a 304k-call spin |

made findable by two diagnostics: the **return-value column** (D459) and **reserve-failure surfacing**
(D462, which also closed the Windows `GetLastError` D010 gap). Plus standard-library stubs: `putchar`,
deterministic `random_device` (D461), `strcpy_s`, `atan2f`, `sincosf`, four `pthread_attr` setters,
`vsprintf_s`, `sceKernelSetVirtualRangeName`, `wcsrchr`, `scePthreadCondattrInit`.

## Where each title now stands, and what it needs

- **PPSA04263**: 32-call death -> **28,343 calls**, walled on an `int 0x41` guest assert
  (`image+0x196b91a`). Needs the assert's read traced to the value we answered wrong - deep RE or an
  obSCEne trace of what real hardware holds there.
- **PPSA21564**: 500,257 calls, `int 0x41` assert (`image+0x11ccd`); flagged gap `sceKernelGetGPI`
  (hardware input). Same assert class as PPSA04263.
- **PPSA25872**: 39,929 calls, spins on the **unnamed** `PS5Util::0xf948d02a4f9f5ace` (side-effect the
  guest polls; forcing its return does nothing). Needs the name (model vocabulary or hardware).
- **PPSA02664 / PPSA03416**: 1,544 calls, walled on `_Getpctype` returning a placeholder the guest
  dereferences as a ctype table. Needs the table off a console (obSCEne).
- **PPSA28061**: 961 calls, needs the GPU (`sceAgcCreateShader`).

So the loop is at the honest edge: every remaining wall needs data this loop cannot produce - an
obSCEne hardware trace, the model's naming vocabulary, or the GPU layer. Stopping here rather than
guessing contracts. A person picking this up should start from the two `int 0x41` asserts (one fix may
free both titles) or point obSCEne at `_Getpctype` (frees two more).
