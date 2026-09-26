# D434 - A fault-triggered backstop re-installs the guest thread pointer when the host has cleared it

**Status:** decided
**Date:** 2026-09-01

On a host that does not preserve a guest-installed thread-pointer base across a context switch, a
fault handler restores it on demand: on an access violation, it re-installs the remembered base
and retries the faulting instruction, but only when the base has actually reverted to its cleared
state.

**Why:** Some hosts reset a guest's thread-pointer base to empty at the next context switch,
where others preserve it for the run; installing it once is not enough on the former. Retrying
only when the base is confirmed reverted, rather than unconditionally, means a genuine fault is
never masked - a cleared base sends every thread-relative access into an unmapped region, so
there is no silent-corruption case this backstop could paper over.

**Rejected:** re-installing the base on every fault regardless of its current value - would
swallow a genuine fault at the same instruction shape.
