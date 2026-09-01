# 2026-09-01 - (/loop) time + fgets: PPSA21564 (Astro's Playroom) reaches main(), prints, parses args

Spine-crunching PPSA21564 toward code-correct headless emulation. Two libc walls in a row:
`printf("%s", asctime(localtime(&t)))` (both time calls unimplemented -> placeholder followed as a %s), and a
`fgets` read loop that never ended because unimplemented `fgets` never answered NULL.

Implemented, correctly:
- `localtime`/`gmtime`/`asctime` in the clock module - real epoch breakdown (Hinnant civil-from-days),
  FreeBSD `struct tm` layout (cited), ISO C `asctime` format, UTC (no timezone modelled). Tested against
  the epoch, a leap day, a recent weekday date, and a pre-epoch time.
- `fgets` on the read-only FILE model - one byte at a time, keeps the newline, NUL-terminates, answers NULL
  at EOF so the guest's read loop ends.

Result (the guest is the oracle): it runs into `main()` and prints its banner, a correct `Current Time`
(our localtime/asctime), the build date, and `ARGS:` read from `/app0/args.txt` (our fgets) with the command
line parsed. Verdict FURTHER; the spin is gone (0% stubs, ~500k real calls). Recorded D454.

New wall is threading: the engine asserts `Cond.cpp:212 rc == 0` and aborts - a condition-variable call
answers non-zero where success is expected (`pthread_cond_wait` -> InvalidHandle/BUSY, or the unserved
`pthread_cond_timedwait`). That is a cond-handle-lifecycle / non-atomic-wait question for a focused turn, and
it is reached only because startup now completes.

`fmt`/`clippy`/tests/knowledge pass for the additions (unsafe split one op per block per the workspace lint).
Additive; other titles unaffected.
