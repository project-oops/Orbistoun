# 2026-09-02 - (/loop) Bulk port batch 9: the attribute accessors, and three guards that fired

```
documented   715 needed, 297 missing   ->   715 needed, 267 missing
```

Fifteen functions, each with its `posix_`-prefixed twin delegated to it (D475), so thirty gap
entries closed.

## What went in

The remaining POSIX list is dominated by attribute accessors, and orbistoun already had the
machinery: `pthread_attr_init` allocates an object **this crate owns the layout of**, and
`attr_get`/`attr_set` read and write fields at fixed offsets. So the work was mostly declaring
new offsets:

- Thread attributes: `getguardsize`, `getinheritsched`, `getschedpolicy`, `getscope`, `setscope`.
- Mutex attributes: `getpshared`/`setpshared`, `getprioceiling`/`setprioceiling`.
- Condition-variable attributes: `getclock`/`setclock`, `getpshared`/`setpshared`, `destroy`.
- `pthread_equal`.

The thread attribute object grew from eight words to sixteen. Safe, and the existing comment
says why: *"this crate defines the layout, defensible only because nothing else reads it"*.

**`pthread_equal` answers non-zero for equal.** That is the opposite sense to `strcmp` and to the
`_np` comparison functions, and a caller reading it as a difference gets every answer backwards -
which is why it is worth a sentence rather than being left to look obvious.

**A note on what these do and do not promise.** Storing `pshared` and `prioceiling` and reading
them back is the *whole* contract of an attribute object - D272 established exactly that, when a
`Gettype` was reading the guest's stack because `Settype` wrote nothing. Whether a lock then
*acts* on the value is the lock's business, and `scePthreadMutexInit` already declares (D474)
that it does not read the attribute block. So these are complete as written; the gap is declared
where it actually lives.

## Three guards fired, and two were my own mistakes

- `every_served_name_is_declared` caught the whole declaration block missing.
- The kernel's `every_implementation_is_also_declared_here_or_says_why_not` caught the same for
  its side. Both exception lists are deliberately **named lists, not blanket exemptions**, so each
  entry went in with its argument written beside it.
- `pthread_equal` turned out to be **already declared with a provisional arity of zero**. POSIX
  gives it two thread identifiers; corrected.

Both of my mistakes were the same one: an idempotence guard keyed on a string that already
existed for another reason - `'"pthread_equal" =>'` matched an old declaration, and
`'"pthread_attr_getguardsize",'` matched the *registration row* rather than the exception list, so
the edit silently did nothing and the tests caught it. A guard string has to be unique to the
thing being guarded, which is now a note in the loop's tooling list.

## State

clippy `--tests` clean, fmt clean, kernel/posix/libc/fs tests pass, identity scan clean, nothing
committed.

**Next**: 267 left. `getsockopt`/`setsockopt` are still deferred and want a per-option table.
Then the two structural items, both worth raising rather than just doing: the **TitleOwn loader**
(unbuilt code, no research, unlocks PS5Util's 36k corpus calls) and the **obSCEne differential**,
which is the only check for "implemented but *wrong*" and has never been run against any of this.
