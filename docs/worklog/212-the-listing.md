# 2026-08-30 - The listing


```text
CWD /  -> 250 Directory changed.
PASV   -> 227 Entering Passive Mode (127,0,0,1,236,244).
LIST   -> 150 File status okay; about to open data connection.

drwxr-xr-x 1 ftp ftp          0 Jan 01 00:00 app0
drwxr-xr-x 1 ftp ftp          0 Jan 01 00:00 data
```

A client browses a directory tree on a PlayStation 5 FTP server running under orbistoun. Three
more things were in the way (D386), and the last one had been wrong since the renderer was
written.

**The root lists its mount points**, because the mount table is the only thing that knows they
exist - `/app0` and `/data` are directories no host directory holds. That turned out to be the
whole of the question that had been sitting open as *what does a console's directory tree look
like*: it was never that question. The tree is data with provenance; what was missing was the
shape, and the shape is knowable from the mount table alone.

**`realpath` answers a guest path**, not the host path it maps to. Handing back
`C:	itles\...` would be a true fact about this machine and a lie about the platform, and the
guest would pass it straight to `open`.

**The seventh argument is on the stack**, and nothing was reading it. `snprintf` spends three
registers on the buffer, the size and the format, so a format with more than three conversions
had nothing left - and the renderer stopped, quietly and correctly, which is why nobody had
noticed. The dispatcher's own documentation said so plainly and had never been connected to
the truncated output it was causing. The trampoline already carried `entry_rsp`, so the
overflow area was one addition away.

**And then `%d` had been reading sixty-four bits.** It is an `int`. The renderer discarded the
length modifier, with a comment saying why that was fine: *every integer argument arrives as a
full register, and the conversion decides how much of it means anything*. True, and doing all
the work - a caller storing an `int` writes `edi`, and writing a 32-bit register zeroes the
upper half of the 64-bit one, so reading all of it was right by accident on every argument for
as long as every argument was a register. The first stack argument ended that, and logged
`RES=-4294967296`.

**The comment was the bug's alibi**: it named a real property, drew a conclusion that happened
to hold, and stopped anybody asking again for as long as it held. The test asserted the
conclusion rather than the property, which made it doctrine.

