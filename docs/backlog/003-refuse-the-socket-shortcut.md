# Refuse the socket shortcut


Recorded here because it is the tempting move and it will look reasonable at the time.
orbistoun opening port 2121 and speaking FTP itself would pass `pros check` in an afternoon
and prove **nothing** - principle 3's plausible output at the scale of a subsystem.

The point of pointing Prosperous at orbistoun is that it is an oracle nobody here wrote and
a stub cannot fool. Every byte it sees has to have been produced by guest code executing.
The same applies to special-casing by payload: `/dev/klog` is a device the platform has, so
emulating the device is the job - noticing the guest is klogsrv and feeding it the trace is
not.

