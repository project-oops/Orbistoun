# D350 - `sysctl` refuses what it does not know, and says what was asked


**decided** · 2026-08-27

`klogsrv` prints its banner and then reports `main.c:278:sysctl`. Two more functions, and
the guest has now named its own requirement precisely.

### Measured, not recalled

`ORBISTOUN_DUMP=sysctl` gives the call:

```
name    = [1, 14, 8, 0]      namelen = 4
oldp    = NULL               oldlenp = <stack>
```

`oldp` null with `oldlenp` set is the first of a pair - **a size query**, asking how large
the answer would be. And the symbol table names the caller:

```
image+0x381  find_pid  (@0x2b0 +209)   called from  main+443
```

So this MIB is how `klogsrv` **finds a process id**. That is a fact about the guest, from
the guest, and it is worth more than a lookup would be: it says what the value is *for*.

**What the MIB means is deliberately not written down.** FreeBSD's `sys/sys/sysctl.h` is not
in the local checkout - only `lib/` is - and mapping `8` to a name by counting entries in
the manual page's list would be inference dressed as a citation. It is recorded as the
number that was asked for, which is checkable, and the name is left open.

### Refusing *is* the implementation

`sysctl(3)`, which **is** in the checkout, documents `ENOENT` as *"The name array specifies
a value that is unknown"*. A documented failure is a real answer: the caller branches on it
and takes its own error path, which is exactly what `klogsrv` does - naming its own file and
line while doing so.

Answering **success** would be much worse. With `oldp` null the caller is asking only for a
length, and reporting success without writing one hands it an uninitialised size to
allocate against.

**errno is left alone rather than guessed.** `ENOENT`'s numeric value is not derivable from
anything lawful here, and the return value is what a caller branches on - the number only
shapes a message. An invented constant is one that gets copied.

Every distinct MIB is reported once, because an unknown one is a work item and the guest is
the only thing that knows which are wanted:

```
orbistoun: sysctl asked for [1.14.8.0] and nothing here knows it - refused with the
documented failure
```

`getpid` answers the **host** process id, which is true rather than invented: the guest runs
in this process, so that is its process id in every sense checkable from inside it.

### Still open, and now well-described

After printing the error, `klogsrv` jumps to null from inside `find_pid`. That is a separate
fault from the `sysctl` failure - the failure is handled and reported, and then something
else goes wrong. One candidate worth checking first: `signal` answers `SIG_DFL`, which is
zero, and a caller that invokes the handler it replaced would call exactly that.



