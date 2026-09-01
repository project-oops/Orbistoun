# D274 - The first call into guest code, rather than out of it


**decided** · 2026-08-25 · what `qsort` needs and no other function did

Every implemented function until now *answers* a call. `qsort` and `bsearch` **make** one:
the caller hands over a function pointer into its own code and expects it to be used. That
is a capability rather than a function, which is why those two were the last checks left in
their section long after the rest of the library worked.

`extern "sysv64"` on the comparator, so the compiler emits the guest's own convention - the
same machinery `orbistoun-abi` already declares for calling a guest entry point. The thunk
dispatch turns out to be re-entrant, so a comparator that itself calls an import lands back
in the dispatcher as an ordinary nested call and nothing had to change to allow it.

`qsort` sorts an index permutation and then applies it, so the comparator is handed addresses
in the guest's own array but the array is not moved underneath it while comparisons run. The
standard makes no such promise; this is stricter than required, not looser.

The same machinery is what a signal handler or a callback-taking system call would need, so
this is worth more than the two checks it closed.

