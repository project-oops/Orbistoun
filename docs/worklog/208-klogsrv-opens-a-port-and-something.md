# 2026-08-29 - klogsrv opens a port and something connects to it


The milestone this project set for itself. `pros check` does one thing per service - a connect
- and a `TcpStream` from outside the process now completes one against a port `klogsrv` opened
while running under orbistoun.

```text
socket -> setsockopt -> bind -> listen -> select
```

`0.0.0.0:3232`, dumped as `00 02 0c a0`. It prints its own address through `getifaddrs` and
`__inet_ntop`, asks for a notification, and waits. With a connection made it keeps working: 26
imports, 62 calls, `strncmp` and `snprintf` six times each, running to the time limit rather
than stopping - which is what a server does.

### What was in the way was ours

The `-1` that survived eight eliminations was in three lines of this project's own reporting
code, and the fault handler had to learn to print a host stack before anything could see it
(D380). The first capture named it in two frames: `first_time_seen`, called from
`orbistoun_syscall_dispatch`. The guest had been reaching the syscall gadget all along;
`vsnprintf` was just the last import it had called before the crash, and the report said
"inside vsnprintf" because that is an attribution rather than a location.

**The dispatcher runs on the guest's stack.** A `BTreeSet` behind a mutex is fine on the
ordinary import path and is not fine here; making it a bitmap moved the fault four lines down
into the `eprintln!`, which formats, allocates and locks. The rule is not "avoid the
container":

> The dispatch path records. The reporting layer prints.

Which is what `call_counts` and `recorded_calls` have always done.

Worth sitting with: the diagnosis needed *three* diagnostics built on top of each other - name
the site (D380), print the host stack (D381), and before either of them, fix the sizing bug
that had been quietly excluding by-name calls from every experiment (D379). Each one was built
because the last one was not enough, and the answer came out in two lines.

