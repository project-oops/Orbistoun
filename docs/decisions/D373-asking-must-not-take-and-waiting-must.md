# D373 - Asking must not take, and waiting must not hold


**decided** - 2026-08-29

`select` is what turns a server's loop: `klogsrv` calls it between `listen` and `accept`, so
without it the loop never turns and the guest takes its error path having served nothing. Two
things about implementing it were not obvious until they were written down.

### Asking must not take

The standard library has no readiness primitive, so the only way to find out whether a
listener has a connection waiting is to **accept one**. Which means the naive `select`
consumes exactly what it was asked to report: the guest is told a connection is ready, calls
`accept`, and waits forever for the next one - having had the first taken by the call that
only asked.

So a listener carries the connection `select` found, and the guest's own `accept` takes it
before it waits on anything. A stream is asked by `peek`, which is a read that does not
advance; nothing is consumed there either.

Zero bytes from a peek is **also ready**, and that matters: an end-of-file is a read that
returns immediately, and a `select` that called it not-ready would park a guest on a
connection that had already closed.

### Waiting must not hold

`accept` blocks, because the interface does. The first version blocked **while holding the
descriptor table**, which is every file call in the process - including the `select` on
another thread that would have been the thing to say a connection had arrived. A guest with
one thread would not have noticed and a guest with two would have deadlocked, in a way that
looked like a hang in guest code.

So the listener is cloned, the table is dropped, and the wait happens outside it. The general
shape: **a lock held across a blocking call is a deadlock waiting for a second thread.**

### And polling is stated rather than hidden

Readiness is asked in a loop a millisecond apart. That costs latency a real `select` would
not, and a guest measuring its own wakeup latency would see it. It is in the knowledge file,
because the alternative - saying nothing - is how a difference like that becomes somebody
else's unexplained measurement later.

