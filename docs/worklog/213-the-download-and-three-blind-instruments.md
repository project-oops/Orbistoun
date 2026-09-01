# 2026-08-30 - The download, and three blind instruments


```text
RETR BackPork_0.1.elf  ->  received 99800 bytes, sha256 d74e4cd1...
```

Which is what the file on disk is. A payload opened it, read it, and pushed it down a passive
data connection to a client outside the process.

The listing beside it was wrong, and every step of finding out why ran into a tool reporting
confidently about something it could not see (D387).

**An empty path is not the root.** `/` and `""` both trim to nothing, so the root synthesised
yesterday answered for both and `stat("")` came back as a directory with two entries in it.
A bug introduced by the fix above it, the same day: the synthesis was one comparison too
generous.

**A path the guest asked for and did not get is now recorded.** The mount table is two entries
wide and what else belongs in it has been an open research question - it is not one, the guest
names them. It settled the question above by staying silent: nothing was failing, so the wrong
answer was one we were giving.

**The dumps could not read a guest thread's stack.** Ranges are published before the guest
starts, when no thread stack exists, so every argument a thread passed dumped as `no region
this run mapped, and address-shaped` - which means *wild pointer* and was an ordinary stack
address. `zftpd` serves every client on a thread, so everything worth looking at was invisible.
One line at thread creation turned that into `/app0`.

**The pattern, three times today**: a tool that cannot see something reports what it *can* see,
in the same words it uses for a real finding. The harvest that did not count what it skipped,
the renderer whose comment explained why a limit did not matter, and the dump window that
stopped where the guest's memory did not. None was careless; each was correct when written and
silently stopped being so.

