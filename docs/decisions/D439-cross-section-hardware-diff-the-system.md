# D439 - Cross-section hardware diff: the system-wide divergences worth fixing


**measured** - 2026-09-01 (user-directed, /loop)

Ran obSCEne's probe under orbistoun and diffed all 262 shared `res` values against the hardware report.
Sorting the ~26 divergences into what transfers and what does not:

**System-wide, fixable (a fixed platform behaviour):**
- File error codes, deferred by the fs layer until measured and now answered: `read`/`write`/`lseek` on a
  bad fd = `-EBADF` (`0xffffffff80020009`), `open` of a missing path = `ENOENT` (`0x80020002`), `open` of
  null = `EFAULT` (`0x8002000e`) - orbistoun returns `-1` for all (`000-boot/write-rejects-bad-fd`,
  `040-file/{lseek,read}-rejects-bad-fd`, `040-file/open-rejects-{missing,null}`).
- `060-module/dlsym-rejects-bad-handle` = `0x80020003` (needs handle validation, D438).
- `005-generation/neo-mode` = `1` (a PS5 is neo-class; orbistoun answers 0).
- `165-gnm/dispatch-direct` writes `6` dwords, orbistoun writes `5`.
- `150-memory-map/{walk,after-allocation}` = `0x8`/`0x9`, orbistoun `0x7`/`0x8` (off by one region).
- The direct-map virtual base `0x2_0000_0000` (`020-memory/map`, `flexible-round-trip`), pending its
  Windows test.

**Not transferable - left alone:** timing and counters (`010-kernel/*`, `050-time/usleep`,
`018-relational/clock`), thread and object identities (`018-relational/*`, `030-thread/self`,
`100-input/open`, `070-user/initial-user` - real ids are large where orbistoun uses `1`, per-run/account),
and the flexible budget (`020-memory/flexible-available`, per-process - D437). `110-modules/*` = `0x20`
vs `1` is a real count divergence (hardware sees 32 loaded modules) but reflects a different loaded set,
not a fixed value.

Fixing the file error codes first, as the largest coherent cluster.

