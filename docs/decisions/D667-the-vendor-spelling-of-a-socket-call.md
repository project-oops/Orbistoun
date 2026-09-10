# D667 - The vendor spelling of a socket call, and the errno that travels with it

**Status:** decided
**Date:** 2026-09-10

## Five checks failed for one reason, and the reason was a table

obSCEne under orbistoun answered `0x7fff0001` to `sceNetSocket`, `sceNetBind`,
`sceNetListen`, `sceNetSetsockopt`, `sceNetRecv` and `sceNetSend`. Every one of those has
had a working body in `orbistoun-fs` for months - under its POSIX name. The vendor twins
were declared in `orbistoun-net` and implemented nowhere, so a **title** reaching the
network hit stubs while a **payload** doing the same thing worked.

The first attempt was to add the vendor names to `orbistoun_fs::socket::implementations()`.
It changed nothing, and the reason is worth recording: that table is consumed by
`orbistoun_posix::implementations()`, which uses it as a *lookup source* for its `DELEGATED`
pairs and returns only POSIX names. Entries added there are visible to the lookup and
invisible to the dispatcher. A table that is read but not returned is a good way to spend an
hour.

## Where a vendor name is offered

**The crate that declares a library offers its names**, wherever the body lives. That is
already D525's rule for `sceKernelStat` - body in `metadata`, name declared and offered in
`orbistoun-fs` - and it decides this cleanly: `orbistoun-net` declares `libSceNet`, so
`orbistoun-net` offers `sceNet*`, and takes a dependency on `orbistoun-fs` for the bodies.

`orbistoun-fs` owns sockets because it owns the descriptor table, and a guest closes a file
and a socket with the same `close`. Splitting the two tables would give a guest two numbering
spaces. So the bodies stay put and the spelling moves.

## The two spellings disagree about exactly one thing

A POSIX-named socket call answers `-1` and leaves the number in `errno`. `libSceNet` folds it
into the return: `0x8041_0100 | errno` (D627). Serving one body under both names means the
success paths agree and the failure paths do not - which is why the vendor names could not
simply be aliased onto the POSIX functions.

So the bodies stopped collapsing to `-1`. Each returns `Answer = Result<u64, u32>` - a value,
or the POSIX errno that stopped it - and each crate encodes it its own way:
`orbistoun_fs::socket::as_posix` and `orbistoun_net::socket::as_vendor`. Two small macros
generate the wrappers, one per crate, because the *rules* differ and neither crate should have
to learn the other's.

**A failure nothing can name is errno zero.** Zero is what `errno` holds when nothing went
wrong, so `0x8041_0100` can never collide with a measured code, and a guest that switches on
the result meets a code it does not know - which is exactly true. Answering a plausible
`EINVAL` would be inventing a constant to make a failure look explained.

## Three things the measurement settled that guessing would not have

**`sceNetSocket` takes a name before the address family.** obSCEne types the import as
`int (*)(const char *, int, int, int)`; POSIX `socket` has no such argument. Sharing the body
unshifted reads a `const char *` where `AF_INET` belongs and refuses every socket. Every
*other* call in this library does sit at the POSIX positions, which is what makes the one
exception easy to miss - and it is now asserted from both sides, because only the pair says
which way round it is.

**`setsockopt(SOL_SOCKET, 0x1200, &1, 4)` is how this platform turns non-blocking on**, and it
is applied rather than accepted. `0x1200` is not the FreeBSD `SO_*` numbering and no harvested
header names it; hardware answered `0x0` to it and `-1` to `fcntl(F_SETFL, O_NONBLOCK)` on the
same machine, and refused the `0x1100` the check offers as a fallback. That it is *applied* is
a second measurement: a recv on that socket answers `EAGAIN`, which a blocking socket cannot
produce. The shim had been accepting it and doing nothing, which is the plausible answer.

**`ENOTCONN` is 57 and it is now a measured entry.** `sceNetRecv` on a listener answers
`0x8041_0139`, on a connected empty socket `0x8041_0123`. Both decode under the base D627
established, and orbistoun now answers the same two codes for the same two conditions.

## `MSG_DONTWAIT` was not a wrong answer, it was a hang

The first run with the vendor names bound got *worse*: 222 imports and 18,383 calls against
245 and 450,179, with the guest silent for the last nine seconds. `102-net/recv-would-block`
connects a plain blocking socket and reads it with `MSG_DONTWAIT`; the shim ignored flags, so
the read waited, and the run's time limit ended the guest with eleven sections unrun.

**A shim that answers wrongly costs one check. A shim that blocks costs the rest of the run.**
That is a distinction worth keeping in view when deciding whether ignoring a flag is
acceptable - and it is why the `Wait` type pairs "force non-blocking for this call" with
"put it back" in one value rather than two statements a reader has to notice are paired.

Putting it back correctly needed the mode remembered on the socket, because the host has a
setter and no getter. Doing that turned up **the same defect already shipped**: the readiness
peek behind `select` set non-blocking, probed, and restored `false` unconditionally - handing
a guest's non-blocking socket back blocking. Found while fixing the other one.

## What it bought

Every `102-net` check now passes, and each one matches the value hardware answered rather than
merely reporting success:

| check | hardware | orbistoun |
|---|---|---|
| `sockaddr-bind` | pass `0x0`, `sin_len` 0 **and** 16 | same |
| `nonblocking-option` | pass `0x1200` | same |
| `recv-would-block` | pass `0x80410123`, listener `0x80410139` | same |
| `accept-inherits` | pass `0x80410123` | same |
| `listener` | pass, descriptor `0x13` | pass, descriptor `0x4` |

The descriptor number is the one honest difference: it is each system's own numbering, and
orbistoun's table starts at 3.

Across the whole sweep, 191 checks passing became 196 with nothing regressing, and two
`900-surface` rows moved from `fail` to `partial`.

## One divergence this opened

Hardware answers **NULL** to `dlsym` for these names; orbistoun now resolves them, because
implementing a name is what makes it resolvable here. Not a failed check - `102-net/resolve`
passes either way - but it is a real difference in what a guest can discover about itself, and
it is recorded rather than left to be rediscovered.
