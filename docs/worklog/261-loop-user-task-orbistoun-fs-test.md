# 2026-09-01 (/loop, user task) - orbistoun-fs test isolation + clippy debt fixed


Root cause of the intermittent sendfile failure and the socket hang was the same landmine:
`descriptor::write` special-cased `fd == 4` as the kernel R/W escape pipe (swallowing the write,
returning success) *before* consulting the table - so once an escape test set the escape address, any
later test whose real file/socket descriptor was 4 had its writes silently discarded. Fixed by checking
the table first: a real descriptor at 4 is written; only an un-tabled 4 (the actual pipe) is swallowed.

Also made `exclusively()` reset the process-wide statics it only serialised before (descriptor, fcntl,
directory, mount tables + the escape address), and added the `exclusively()` guard to the socket and
fcntl tests that touch shared state without it (plus a bounded connect/read timeout on the socket test
so it can never hang). 14+ consecutive clean full-suite runs.

Cleared the escape.rs/socket.rs clippy debt in the same pass: long-literal separators, a split of the
two-unsafe-op setsockopt block with real SAFETY comments, and a refactor of `read_kernel_pipe` (135->
<100 lines) by extracting `dynlib_obj` (the three near-identical dynlib_obj blocks) and a `copy_from`
helper (the per-region copy tail). Also fixed the clippy regression my write-match change introduced.
orbistoun-fs clippy is clean; escape tests still pass (behaviour preserved).

