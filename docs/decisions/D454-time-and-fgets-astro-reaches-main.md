# D454 - localtime/asctime and fgets carry PPSA21564 into main(); it prints, parses its args, and walls on a threading assert


**measured** - 2026-09-01 (user-directed /loop; spine-crunching a single title toward code-correct headless emulation)

Following the TLS-key work (D453), PPSA21564's next wall was `printf("%s", asctime(localtime(&t)))`:
`localtime` and `asctime` were unimplemented, so `localtime` answered the placeholder, `asctime` passed
it on, and `printf` followed the `%s` into `0x7fff0001`. Behind that lay a second wall - a `fgets` read
loop that, unimplemented, never answered the NULL that ends it and spun to the call budget.

**What was implemented, correctly rather than plausibly.**

- `localtime`/`gmtime`/`asctime` in `orbistoun-libc`'s clock module: a real seconds-since-epoch breakdown
  (Howard Hinnant's civil-from-days, exact across the whole range), the FreeBSD `struct tm` layout (nine
  ints then `tm_gmtoff`/`tm_zone`, cited from `<time.h>`), and ISO C's fixed `asctime` format. No timezone
  is modelled, so local time is UTC - the one honest answer available. Tested against the epoch, a leap day,
  a weekday-bearing recent date and a pre-epoch time, so the maths is checked and not merely present.
- `fgets` on the existing read-only FILE model: one byte at a time, keeping the newline, NUL-terminating,
  and - the point of it - answering NULL at end of file so the guest's `while (fgets(...))` loop ends.

**The result, and it is visible.** PPSA21564 is Astro's Playroom (`ASOBI PlayRoom`), and it now runs into
`main()` and prints, in its own words:

```
========main()=========
ASOBI PlayRoom
Current Time    : Tue Sep  1 17:55:40 2026     <- our localtime/asctime, a correct date
Build_Date Time :  Oct 10 2024 16:34:14
ARGS: file: /app0/args.txt                     <- our fgets, reading a real file
ARGS: line: -package -sequence product -mode product ...
```

That the timestamp is right and the argument file is read back verbatim is the confirmation the two pieces
work - the guest is the oracle. Verdict FURTHER; the boot no longer spins (0% on stubs, ~500k real calls).

**The new wall is threading, not libc.** The guest then trips its own engine assertion -
`D:\asobi\6.0\source\engine\app\Common\Conc\Cond.cpp:212: rc == 0` - and faults in the abort path. A
condition-variable call is answering non-zero where the engine asserts success: `pthread_cond_wait` answers
`InvalidHandle` for a cond it does not recognise or `BUSY` for a missed signal, and `pthread_cond_timedwait`
is declared but unserved. Which one, and why the handle or the signal is lost, is a threading-correctness
question for a focused turn - the cond/mutex primitives' handle lifecycle and the non-atomic wait gap
(recorded on `scePthreadCondWait`) are the places to look. It is reached only because startup now completes.

`fmt`/`clippy`/`cargo test`/the knowledge audit pass for the additions; the unsafe blocks are split one
operation each per the workspace lint. Additive - new libc symbols - so other titles are unaffected.
