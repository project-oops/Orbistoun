# D387 - A guest thread's stack is guest memory, and the diagnostics could not see it


**assumed** - 2026-08-30

A file comes out of the guest, byte for byte:

```text
RETR BackPork_0.1.elf -> 150 File status okay; about to open data connection.
  received 99800 bytes, sha256 d74e4cd1...
                      -> 226 Closing data connection. Transfer complete.
```

`99800` and `d74e4cd1...` are what the file on disk is. That is `open`, `read`, a passive data
socket and a transfer, driven end to end by a payload.

The listing beside it was wrong, and finding out why took three separate instruments that were
all reporting confidently about things they could not see.

### An empty path is not the root

Every entry listed as `drwxr-xr-x`, size zero, `Jan 01 00:00` - a directory, whatever it
actually was.

`mounts_under` normalised a path by trimming trailing slashes, and `/` and `""` **both trim to
nothing**. So the synthesised root (D386) answered for the empty path too, and `stat("")` came
back as *a directory with two entries in it*. `zftpd` stats every name in a listing and passes
an empty one for each; on a real system that fails and the server falls back to `d_type`, and
here it succeeded.

The rule is now written the other way round: a path that does not begin at the root names
nothing. That covers the empty path and the relative one together, which is right for the same
reason - **nothing here has a working directory**, which `getcwd` already reports, and a
relative path silently resolving against the root would be a second answer to a question
already answered.

This was a bug introduced the same day by the fix above it. The root needed synthesising and
the synthesis was one comparison too generous.

### A path the guest asked for and did not get is a work item

The mount table is two entries wide and what else belongs in it has been an open research
question. **It is not one. The guest names them.** An FTP server asked to list a directory
calls `stat` on it; a title asked for a save calls `open`. Every failure is a path something
real wanted, spelled by the thing that wanted it - which is a measurement, and the only kind
of evidence this project takes.

So every resolution that fails is recorded, once per distinct path, and the run says so when
it stops. A mount added afterwards then answers a request that was actually made.

It also settled the question above by *staying silent*: no paths were reported, so `stat` was
not failing, so the wrong answer was one this project was giving confidently.

### The dumps could not read a thread's stack

The readable ranges are published once, before the guest is entered: the image and the main
stack. **A guest thread's stack does not exist yet at that moment.**

`zftpd` serves every client on a thread, so every argument worth looking at lived on one - and
every dump of one came back as `no region this run mapped, and address-shaped`. That phrase
means *a wild pointer*. It was an ordinary stack address, and the tool was reporting the wrong
kind of thing about its own blind spot, which is principle 3 one level up from where it
usually bites: the same shape as the readable window that was a page too low and turned a
pointer into a count (D217), and as the four other guards that reported more than they
measured.

A thread now publishes its stack the moment it has one, into a fixed array of atomics -
allocation-free and lock-free, because a dump runs on the guest's own stack (D381). The first
run after it turned `no region this run mapped` into `/app0`.

**Every instrument in this project has a range of validity, and none of them said so.** Three
today: the harvest that did not count what it skipped (D385), the format renderer whose
comment explained why a limit did not matter (D386), and this. The pattern is not carelessness
- each was correct when written. It is that a tool which cannot see something reports what it
*can* see, in the same words it uses for a real finding.

