# 2026-08-30 - An FTP server, and the six things in front of it


```text
$ python probe.py
connected to 127.0.0.1:2120
220 Service ready for new user.
230 User logged in, proceed.
215 UNIX Type: L8
200 Type set.
```

`zftpd` runs under orbistoun and serves FTP to a client outside the process. `klogsrv`
accepts on 3232 and `shsrv` on 2323, and shsrv drops the connection because it wants to
`exec` a shell, which is the honest next wall.

Six things stood in the way and each was a different kind of mistake (D384, D385).

**A gadget is not reached from a call site a compiler wrote.** The `0xffffffffffffffff` fault
that survived two sessions was not a wild pointer - it is what Windows reports for a
general-protection fault, and the instruction was `movaps [rbp-0x30], xmm0` on a stack eight
off. Everything else reaches this project through a relocation, so it arrives aligned and the
run says so on every boot. A gadget is reached through a pointer the guest keeps, and `ftpsrv`
arrives misaligned. The arithmetic had a comment explaining why it was right, and the comment
was an assumption about the guest written as a fact about the instruction set.

**Making two things agree by construction is not making either right.** `libScePosix` takes a
POSIX call's arity from the vendor twin it delegates to, "so the two cannot disagree". Three
vendor calls end in a name the POSIX ones have no argument for, so a guest got an
uninitialised register read as a string pointer. It worked for every guest that happened to
leave a readable address there.

**A diagnostic that changes the program is not a measurement**, which principle 3 already
says. Entering past the runtime marks every unserved `.bss` global non-zero so its first use
names it - right for a payload with a handful of loader-supplied kernel addresses, wrong for
one with 126 globals. `zftpd` printed `Server running` and `Shutting down` in the same breath
with no call in between. The function's own doc comment said it left them null; the code did
the other thing, and had for as long as both existed.

**What a harvest skips has to be counted.** `sys/sys/event.h` writes every filter as `(-1)`,
the harvester required bare digits, and it took `EVFILT_SYSCOUNT` and nothing else - the one
number of the set that is not a filter. Third time a spelling rule silently took the wrong
set, and every time the count was the only thing that said so. The harvest now names what it
skipped, per section, which immediately showed `INADDR_ANY`, `SIG_IGN` and fifty-four
interface flags sitting outside the table.

The edge test made it worse: it asserted `!is_plain_number("-1")`, written from the rule
rather than from what a header contains, so **the test protected the bug**.

**A number is read where it is used, or it is retyped.** `abi_constant` lived above
`orbistoun-fs`, so the crate implementing sockets wrote `AF_INET = 2` by hand with a citation
in a comment, checked by a test comparing two copies. The table moved down to `orbistoun-hle`
and there is one copy.

And `fcntl`, `kqueue`/`kevent`, `inet_pton` and the sixteen-byte address family, each found by
the payload stopping on it and saying so in its own words.

