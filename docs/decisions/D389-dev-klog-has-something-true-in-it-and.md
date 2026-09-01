# D389 - `/dev/klog` has something true in it, and the accuracy caveat that comes with it


**assumed** - 2026-08-30

`klogsrv` exists to forward `/dev/klog` to a socket. Under orbistoun it binds its port, accepts
a connection, and has nothing to send: the payload works and the device does not exist.

It does not have to be invented. `/dev/klog` is where a FreeBSD kernel tells the programs
running on it what it is doing, and **orbistoun is the kernel those programs are running on**.
Every line it already writes about a guest - a call it could not serve, a name nothing
implements, a path it does not hold - is exactly that.

So there is a bounded ring of lines, fed from the reporting layer, and a device that serves
them. It is a character device, read-only, and *empty means not ready* rather than end of
file - a kernel log has no end while the kernel is running, and a guest told otherwise stops
reading.

### The caveat, stated rather than left to be discovered

**The device is faithful; the content is not the console's.** A real `/dev/klog` carries the
PlayStation kernel's own messages. This carries orbistoun's. A guest that *parses* klog output
- looking for a driver's name, a firmware string, a known format - will read something with
the right shape and the wrong words.

It is still worth having: the alternative is a device that is absent, which sends `klogsrv`
down an error path over something orbistoun can answer honestly. But it must not be counted as
fidelity, and a guest that behaves differently because of what it read here is a run to be
suspicious of.

A device is not a mount, so `/dev` is answered by the device layer instead: `/dev` is a
directory holding the devices, `/` holds `/dev`, and a `stat` of the device reports `S_IFCHR`
rather than a regular file of size zero - which is what a caller checking before it reads is
actually asking.

