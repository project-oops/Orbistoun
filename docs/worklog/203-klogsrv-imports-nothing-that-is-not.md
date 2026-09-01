# 2026-08-29 - klogsrv imports nothing that is not implemented


Kept going down the measured list rather than stopping to decide what to do next, which is
what the list is for.

**klogsrv is done.** `select`, `getifaddrs`, `freeifaddrs`, `__inet_ntop`, `kill` and the
notification call were the last six, and its only remaining unresolved names are two data
objects that already have storage. Sixteen missing this morning, none now.

**ftpsrv went from 48 to 7**, five of which are functions: `pread`/`pwrite`, `dup2`,
`chmod`, `mlock`, the underscored aliases, `stat`/`lstat`/`fstat`, `opendir`/`readdir`/
`closedir`, `fdopen`/`fileno`, `sendfile` all landed. It wants `sscanf` and `strftime`.

Two payloads now import nothing missing at all.

### The things that were not obvious

**`select` must ask without taking.** The only way to learn whether a listener has a
connection is to accept one, so a naive `select` consumes exactly what it reports. The
connection is kept on the listener now and the guest's own `accept` takes it. And `accept`
must not block while holding the descriptor table, or the `select` on another thread that
would have reported the connection can never run (D373).

**Windows' positioned read moves the file pointer**, which is precisely what `pread` promises
not to do. Found by a test reading three bytes from the wrong place. The position is put back
under the descriptor lock - which is the atomicity the syscall does not give.

**`fdopen` is only real if `fprintf` follows it.** A server wraps an accepted connection and
writes its replies through the stream; a `fprintf` that ignored the stream would send every
reply to the host's error stream and look like a working server nobody could talk to.

**The checkout is a newer FreeBSD than the target** (D374). Nine harvests in, `stat` and
`dirent` are the first structures where that matters: both changed shape after release 11.
Writing the modern layout would report the wrong size for every file with nothing saying so.
Both layouts are in the same header, so the choice is a setting - and the constants file's
existing caveat, *"FreeBSD's numbers, not the target's"*, turns out to have a second axis
nobody had written down: the target is a fork of a *particular release*, and a number stable
across releases and a structure that is not are different kinds of borrowing.

**C octal is not TOML octal.** `S_IFDIR` is `0040000` in the header; TOML rejects a leading
zero, so the first file mode harvested made the whole table unparseable - and it surfaced as
every constant in every section going missing at once, which reads like a build problem
rather than like one number.

