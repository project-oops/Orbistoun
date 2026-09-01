# D385 - What a harvest skips has to be counted, or the section is a lie


**assumed** - 2026-08-30

`zftpd` serves FTP under orbistoun. A client connects, logs in, and gets answers:

```text
220 Service ready for new user.
230 User logged in, proceed.
215 UNIX Type: L8
200 Type set.
```

Six things stood in the way. Each is worth its own paragraph, and one of them is a rule about
this project's own tooling rather than about the platform.

### The harvester had taken none of the event filters

`sys/sys/event.h` writes every filter as `#define EVFILT_READ (-1)`, which is what a header
does with a negative constant so `EVFILT_READ - 1` cannot mean something else. The harvester
required bare digits, so it took **`EVFILT_SYSCOUNT` and nothing else** - the one number in
the set that is not a filter. The section existed, looked harvested, and named nothing a guest
can ask for.

That is the **third** time a rule about spelling silently took the wrong set:

| rule | what it dropped | how it surfaced |
|---|---|---|
| upper case throughout (D378) | 599 of 600 syscall numbers | the count |
| C octal is not TOML octal (D374) | every constant in every section | a parse failure |
| bare digits only | all fifteen event filters | the count |

Three times, and each time **the count was the only thing that said so**. So the rule is now:
a harvest reports what it skipped, by name, per section. A `#define` whose name qualifies and
whose value does not is a decision, and a decision nobody can see is one nobody can check.

The edge test made it worse rather than better. It asserted `!is_plain_number("-1")` - written
from the rule rather than from what a header contains - so the test *protected* the bug. A
test that restates the implementation is not a test.

### The table was in a crate that could not be reached from underneath

`abi_constant` lived in `orbistoun-libc`, which depends on `orbistoun-fs`. So the crate that
implements sockets, files and now event queues could not read the harvested numbers, and wrote
its own down by hand with a citation in a comment - `pub const AF_INET: u64 = 2;` - checked by
a test in the crate that *could* read the table, comparing two copies.

That is the retyping the harvest exists to prevent, wearing a comment as a disguise. The table
moved to `orbistoun-hle`, which is below both, and the families are read where they are used.
The test comparing two copies is gone because there is one copy, and what replaced it asserts
the reader rather than the agreement.

### A POSIX name and its vendor twin can have different arities

`libScePosix` delegates most POSIX names straight to their vendor-named twin, and takes the
arity "from the vendor-named function each delegates to, so the two cannot disagree". They
disagree. Three of the vendor calls end in a **name** the POSIX ones have no argument for:

| POSIX | vendor |
|---|---|
| `pthread_create(thread, attr, start, arg)` | `scePthreadCreate(..., name)` |
| `pthread_cond_init(cond, attr)` | `scePthreadCondInit(..., name)` |
| `pthread_mutex_init(mutex, attr)` | `scePthreadMutexInit(..., name)` |

So a guest calling the POSIX spelling had an **uninitialised argument register** read as a
string pointer. `zftpd` had bound its socket and listened on it and was initialising its
client table when `pthread_mutex_init` read `rdx`, which held `0x18` left over from the loop
above, and faulted on it.

Nothing detects this by inspection. The delegation resolves, the test that every delegation
names a real implementation passes, and the call works perfectly for every guest that happens
to leave a readable address in that register. **Making two things agree by construction is not
the same as making either of them right.**

### `fcntl` is a pair, which makes an unimplemented answer worse

```text
fcntl(5, F_GETFL)            -> 0x7fff0005    an orbistoun placeholder
fcntl(5, F_SETFL, 0x7fff0005)                 handed straight back
close(5)
```

D125 in its purest form: a function answering an error code where a caller expects data, and
the damage landing one call later under a different name. A guest reads flags, changes one
bit, and writes them back, so a placeholder does not stay where it was put.

`O_NONBLOCK` is honoured because a server's event loop depends on it. `FD_CLOEXEC` is
remembered and does nothing, which is honest rather than lazy: nothing here ever `exec`s, so
there is no moment at which it could have an effect, and a guest that sets it and checks it is
still entitled to a consistent answer.

### A marker in a `.bss` global is a diagnostic changing the program

Entering past the runtime fills the globals that runtime would have filled, and gives every
*unserved* one a marker instead of the null the loader left - so its first use names it rather
than faulting on zero. That is the right answer for a payload whose unserved globals are a
handful of kernel addresses a loader supplies.

`zftpd` has 126 named globals and 24 that nothing here implements. It printed
`Server running. Press Ctrl+C to stop.` and then `Shutting down...` in the same breath, having
made no call in between - because a marker is non-zero, and one of those globals is read by a
`while`.

**A diagnostic that changes the program is not a measurement** (principle 3, D227). The
function's own doc comment claimed it left unserved names null; the code did the other thing,
and the two had disagreed for as long as both existed. `ORBISTOUN_RUNTIME_GLOBALS=zero` now
means what the comment said, and under it `zftpd` stays up and serves.

### Where the five core payloads stand

| payload | port | how far |
|---|---|---|
| `zftpd` | 2120 | **serves** - banner, login, `SYST`, `PWD`, `TYPE` |
| `klogsrv` | 3232 | **accepts** a connection and holds it |
| `shsrv` | 2323 | **accepts**, then drops: it wants to `exec` a shell |
| `ftpsrv` | 2121 | stops at the AuthID wall - it raises kernel privileges first (D382) |
| `elfldr` | 9021 | dies in `__crt_start`; no `main` symbol to enter at (D326) |
| `pldmgr` | 8084 | the same |

`CWD /` answers `550 Not a directory.`, which is the filesystem's root: `/` is no mount
prefix, so the first thing an FTP client does has nowhere to go. That is now a measured work
item rather than a question about what a console's directory tree looks like - the server will
say which paths it wants, in order, as soon as it can list one.

