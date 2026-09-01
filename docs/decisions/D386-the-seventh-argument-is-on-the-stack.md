# D386 - The seventh argument is on the stack, and nothing was reading it


**assumed** - 2026-08-30

A client lists a directory on a PlayStation 5 FTP server running under orbistoun:

```text
CWD /  -> 250 Directory changed.
PASV   -> 227 Entering Passive Mode (127,0,0,1,236,244).
LIST   -> 150 File status okay; about to open data connection.

drwxr-xr-x 1 ftp ftp          0 Jan 01 00:00 app0
drwxr-xr-x 1 ftp ftp          0 Jan 01 00:00 data

       -> 226 Closing data connection. Transfer complete.
```

Three things stood between the login (D385) and that, and the third is the one that had been
quietly wrong for the whole project.

### A root that lists its mount points

`/app0` and `/data` are directories a guest can enter and **no host directory holds them**:
they are prefixes this project maps onto host roots that live somewhere else entirely. So
`resolve("/")` answered nothing, `opendir("/")` found nothing, and `CWD /` - the first thing
an FTP client does - was refused with `550 Not a directory.`

The mount table is the only thing that knows those names exist, so it says so:
`mounts_under("/")` answers `app0` and `data`, and a mount at `/system_data/priv` would make
`/system_data` a directory too, holding `priv`. **Only the next component**, never a whole
prefix, because that is what a directory entry is.

`stat` gets the same treatment, with zero size and zero times - it is not a directory on any
disk, so there is no modification time to report and inventing one would be a fact about
nothing.

This is the answer to a question that had been sitting open as *what does a console's
directory tree look like*. It is not that question. The tree is data with provenance, and
what was missing was the **shape** - and the shape is knowable from the mount table alone.

### `realpath`, which is how a server decides a path is real

`CWD /` then moved from `550 Not a directory.` to `550 Invalid path.` - a different refusal,
which is what progress looks like. `zftpd` canonicalises every path a client names, got the
placeholder back, and refused it.

The answer is a **guest** path. Handing back `C:\titles\PPSA00000` would be a true fact about
this machine and a lie about the platform, and the guest would pass it straight to `open`. So
the components are walked here rather than by `std::fs::canonicalize`, and `..` above the root
stays at the root - which is what every filesystem does and what stops a path leaving the
mount table by spelling alone.

### The seventh argument, which was never being read

With the root browsable, the replies came back mutilated:

```text
PWD  -> 257
PASV -> 227
[FTP][INFO]
```

Every one truncated at its first conversion. `227 Entering Passive Mode (%d,%d,%d,%d,%d,%d)`
is a format, a buffer, a size and six numbers - **nine arguments**, and System V passes six in
registers. `snprintf` spends three of those on the buffer, the size and the format, so three
were left for six conversions and the renderer stopped when it ran out.

It stopped *quietly*, which is correct behaviour for a renderer whose arguments have run out
and is exactly why nobody had noticed. The dispatcher's own documentation said so plainly -
*the seventh argument onwards is on the guest stack and is not captured here* - a known gap,
written down, and never connected to the truncated output it was causing.

The trampoline already carries `entry_rsp`, so the overflow area is one addition away:
`[entry_rsp]` is the return address the guest's `call` pushed, and the arguments that did not
fit start at `[entry_rsp + 8]`. That is the `overflow_arg_area` the psABI defines, reached
from the other side. It is published for the length of the call in a thread-local, saved and
restored so a nested call cannot blind the outer one.

**What it does not fix**: the count is still the format string's word. Reading past what the
guest passed gives whatever the stack held - the same risk a real `printf` has, for the same
reason, bounded here at sixty-four words so a wrong format stops rather than walks.

### And then the width, which had been wrong all along

The first thing to read a stack argument logged `RES=-4294967296`. That is
`0xFFFF_FFFF_0000_0000`: a zero with somebody else's bits above it.

`%d` is an `int`. Thirty-two bits. The renderer had been reading sixty-four and discarding the
length modifier, with a comment explaining why that was fine - *every integer argument arrives
as a full register, and the conversion decides how much of it means anything*.

The first half was true, and it was doing all the work. A caller storing an `int` writes
`edi`, and **writing a 32-bit register zeroes the upper half of the 64-bit one** - so reading
all sixty-four bits gave the right answer by accident, on every argument, for as long as every
argument was a register. An argument on the stack sits in an eight-byte slot whose upper half
is unspecified for anything narrower, and the accident stopped.

So the modifier is tracked rather than discarded: `int` by default, `h`/`hh` narrower, `l`,
`z`, `j`, `t` the whole word, and a pointer always the whole word. `%u` of `-1` is
`4294967295` and `%lu` is `18446744073709551615`, which are different questions about the same
register and must not answer the same.

**The comment was the bug's alibi.** It named a real property, drew a conclusion that happened
to hold, and stopped anybody asking again for as long as it held. A test asserted the
conclusion rather than the property - *a length modifier is consumed and changes nothing* -
which made it doctrine.

