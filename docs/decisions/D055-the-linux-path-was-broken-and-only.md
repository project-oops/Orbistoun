# D055 - The Linux path was broken, and only running it showed that

**decided** · 2026-08-19

D027 says neither platform ships an unverified implementation with a claim attached.
Windows passed all four reservation tests; **Linux failed all four**, and would have
shipped with a false claim had it not been run in the multipass VM.

Two bugs, the second worse than the first:

- **`MAP_PRIVATE` was missing.** An `mmap` with neither `PRIVATE` nor `SHARED` is
  `EINVAL`, so every reservation failed. A pure oversight, invisible on Windows
  because that path is separate code.
- **Every `mmap` error was reported as `Conflict`.** So the `EINVAL` surfaced as
  "range taken", sending a reader hunting for a phantom occupant instead of at the
  wrong argument that actually caused it. Errors are now distinguished: `EEXIST` and
  `ENOMEM` are conflicts, everything else reports the errno.

The second is the one worth remembering. A wrong error message is worse than no error
message, because it is actively misleading - the same failure D010 exists to prevent,
arriving through error mapping rather than through a stub.

Verified: 8/8 on Linux and 8/8 on Windows, same source.

**Method note for future platform work.** Only `orbistoun-mem` was built in the VM,
with `CARGO_TARGET_DIR` pointed at VM-local storage so it could not collide with the
Windows build directory over the mount. The VM was unmounted and its build artifacts
removed afterwards. It is a working CI runner, not a scratch box.

