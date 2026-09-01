# D382 - ftpsrv wants to be root, and that is a wall worth having


**decided** - 2026-08-29

With `klogsrv` listening (D381), the measured list said `ftpsrv` was **two functions** away:
`sscanf` and `strftime`. Both are written now, and `ftpsrv` imports nothing this project does
not implement.

It does not serve. It gets further than that: it runs, prints its own diagnostics in its own
words, and stops on something orbistoun has no answer for.

```text
main-prospero.c:49:malloc: error 0 (orbistoun has no message table)
Unable to change AuthID
```

**It is trying to become root.** The `KERNEL_ADDRESS_*` globals a payload keeps - `ALLPROC`,
`ROOTVNODE`, `PRISON0`, `SECURITY_FLAGS` - are kernel addresses a loader supplies on real
hardware, and `ftpsrv` uses them to raise its own privileges before it serves a single file.
There is no kernel here to have them, and the honest answer is the one it got.

That is a **stated wall rather than a mystery**, which is the whole difference. `klogsrv`
listens because a log server needs a socket; `ftpsrv` needs a filesystem it is allowed to read,
and asks for it the way a jailbroken console lets it.

### The two functions

`sscanf` is `printf` run backwards and inherits the same limit: six argument registers, two
spent on the string and the format, so four conversions can be assigned and a fifth cannot. A
format needing more is **refused entirely** rather than partly performed - the count is
contractually "how many succeeded", so a caller told four believes those four are good.

`strftime` needs `struct tm`, which is nine `int`s and then a `long`, from `include/time.h`. A
conversion it cannot render stops the whole call: a half-rendered timestamp is a wrong date
rather than a short one, and a caller printing it cannot tell.

### An enumeration of processes has a true answer

Both payloads ask `sysctl` for `kern.proc.proc` - looking for an earlier copy of themselves.
`klogsrv` takes the refusal and carries on; `ftpsrv` **exits**, which is a correct program
handling a call that failed for a reason it cannot interpret.

Nothing here has a process table, and that is not a gap to paper over - it is the answer. The
call succeeds and reports a zero-length result, which a caller reads as *none*, and no process
it could be looking for is running. It also avoids `struct kinfo_proc` entirely, which is one
of the structures whose layout moved between the release this project harvests and the one the
target forked from (D374).

### And a check that was too clever, withdrawn

D380 made `%s` follow a pointer only if it was inside a range the run had published. Those
ranges are the **guest's** - its image and its stack - and a `%s` argument is very often a
pointer into memory *this project* handed the guest: a `strerror` buffer, a `getifaddrs` block.
So the check refused them, and `ftpsrv` printed a perfectly good error message with
`(unmapped)` where the reason should have been.

It is back to the narrow rule, which is what was actually needed: null, the null page and
all-ones are not addresses any program computed; everything else is followed and faults as the
machine would. **The wrong version of that check was visible in a guest's own output for about
an hour**, which is the cheapest possible way to find out it was wrong, and only because a
payload had got far enough to print.

