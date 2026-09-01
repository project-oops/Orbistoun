# D381 - The dispatch path runs on the guest's stack


**decided** - 2026-08-29

`klogsrv` opens a listening socket on port 3232 under orbistoun, and something outside the
process can connect to it. That is the milestone this project set for itself - `pros check`
does exactly one thing per service, and it is that connect.

What stood between here and there was three lines of this project's own reporting code.

### The backtrace that ended a long afternoon

A `-1` read had survived eight eliminations. The fault reported itself as being inside
`vsnprintf` - which is an *attribution*, the last import the guest called, not a location
(D380) - and every pointer into that path had been guarded without moving it.

So the fault handler learned to capture the host stack when the fault is in orbistoun's own
code. The first capture:

```text
11: orbistoun_thunk::syscall::first_time_seen
12: orbistoun_thunk::syscall::orbistoun_syscall_dispatch
```

The guest **was** calling the syscall gadget. The crash was in the reporting inside the
dispatcher, and `vsnprintf` was simply the last thing the guest had called before it.

### Why the dispatcher must not allocate, print, or lock

A guest calls the gadget; the gadget calls the dispatcher; **every frame from there down is on
whatever stack the guest was using.** That is a stack this emulator did not size and does not
own.

The first version kept a `BTreeSet` behind a mutex - exactly as the `sysctl` and `dlsym`
reports do, and those are fine, because they are reached from the ordinary import path on a
frame this project arranged. This one is not. It faulted inside `BTreeSet::insert` on the
first syscall a guest ever made here.

Making it a bitmap of atomics removed the allocation and moved the fault four lines down, into
the `eprintln!` - which formats, allocates and takes a lock, all on the same stack. So the rule
is not "avoid the container", it is:

> **The dispatch path records. The reporting layer prints.**

Which is what `call_counts` and `recorded_calls` have always done, and this had simply not been
built that way.

### What was on the other side

With nothing printing from the dispatcher, `klogsrv` went from 10 imports and 19 calls to 23
and 39, and stopped faulting entirely - it ran to the time limit, which is what a server does.

```text
socket -> setsockopt -> bind -> listen -> select
```

It binds `0.0.0.0:3232` - the address dumped as `00 02 0c a0`, which is `AF_INET`, port 3232,
`INADDR_ANY` - prints its own interface address through `getifaddrs` and `__inet_ntop`, asks
for a notification, and waits in `select` for a connection.

A `TcpStream` from outside the process connects to it.

### What it is not

It is not a clean run of a payload. It is entered past its own runtime, with the globals that
runtime would have filled written in by the loader (D376) and the handoff structure's
unestablished fields still markers. That mode declares itself, and every fill is reported.

What is real is the socket: the guest asked for it, bound it, listened on it, and something
outside connected. No part of that was faked, and `orbistoun never implements FTP` has its
counterpart here - orbistoun never opened this port, `klogsrv` did.

