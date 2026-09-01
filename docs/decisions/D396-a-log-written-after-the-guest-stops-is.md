# D396 - A log written after the guest stops is a log no guest can read


**assumed** - 2026-08-30

`/dev/klog` existed, `klogsrv` opened it, a client connected - and got nothing, every time.

The device was right. The **feed** was wired to the reporting layer, which runs after the guest
has stopped: the paths it could not answer, the syscalls it was asked for, the names it could
not resolve. All of it correct, all of it written to a device whose only reader had already
been told the run was over.

Which is the same mistake as the report that could only fire before the guest started (D387),
one day and one subsystem apart: **a record and its reader have to be alive at the same time**,
and nothing about the code says when either of them runs.

So the kernel-boundary events are written where they happen - a path resolved to nothing, a MIB
nothing knows - and the run's own opening lines are written before the guest starts, which is
what a kernel log has at the top of it anyway:

```text
$ nc 127.0.0.1 3232
orbistoun: presenting a ps5/cex/base machine
orbistoun: placed 3 segments (77148 copied, 2332 zeroed) at 0x400000000000
orbistoun: relocations 175/175 applied (0 TLS-deferred, 0 unsupported, 0 unresolved)
```

That is `klogsrv` doing the whole of its job: forwarding `/dev/klog` to a socket, with something
true on the other end.

### The one that still cannot be written from where it happens

Syscalls. The dispatcher runs on the guest's stack and must not allocate or lock (D381), so a
direct syscall still only reaches the log in the closing summary. That is a real gap and it is
the honest one: the alternative is a kernel log that faults the kernel.

### The caveat from D389 is unchanged and worth repeating

The device is faithful; the content is orbistoun's, not a console's. A guest parsing klog output
for a driver's name or a firmware string reads the right shape and the wrong words.

