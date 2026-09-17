# 615. Opening a directory returned ENOENT on Windows, and it walled Grand Theft Auto at `int 0x41`

**2026-09-15** - the reclassified `int 0x41` finding pointed upstream, and upstream was a host-platform
bug: a guest could not open its own `/app0` directory on Windows

## How the taxonomy handed me the cause

int 0x41 is now a measured-fatal guest trap whose cause is upstream (D699, worklog 611), and the
finding routes to the call before it. On PPSA04263 (Grand Theft Auto V) that call was named, plainly:

```
! the guest executed int 0x41 at image+0x196b91a - measured a fatal trap on retail ...
just before: sceKernelOpen(0x6100087ffff0) -> 0x80020002   <- the gate
just before: libc::strlen(0x6100087ffff0) -> 0x6
  -> ... the guest reached it via an upstream wrong value. Read the calls just before it
```

`strlen` answered `6`, the path register pointed at `"/app0/"` (six characters), and `sceKernelOpen`
answered `0x80020002` - the vendor `ENOENT`. The run's own work-item line said it outright: *"the
guest asked for paths nothing here holds: /app0, /host//ap/rpf.cache."* So the guest opened its **own
`/app0` mount root** and orbistoun said it did not exist.

## Why it did not exist - a host-platform bug, not a mount gap

`mount::resolve("/app0/")` resolves fine: the trailing slash trims to empty and it answers the host
`/app0` root, which exists. The failure was one line later, in `descriptor::open`:
`std::fs::File::open(host)`. **On Windows `File::open` fails on a directory** - opening one needs
`FILE_FLAG_BACKUP_SEMANTICS`, a documented Win32 flag - so the open errored, was recorded as a wanted
path, and answered `None`, which `kernel_open` turns into `ENOENT`. On Unix the same `File::open`
opens a directory read-only and none of this happens, which is why the bug was invisible until a title
opened a directory on a Windows host.

A guest opening `/app0` to check its own directory gets a descriptor on the console. So it must here,
on both host platforms.

## The fix

`open_readable(host)` replaces the bare `File::open`: on Windows it adds
`FILE_FLAG_BACKUP_SEMANTICS` when the host path is a directory (via `OpenOptionsExt::custom_flags`),
and on Unix it is the plain `File::open` that already worked. A directory now opens to an ordinary
`Target::File` - a read-only handle that `fstat` reports as a directory and `read` answers zero bytes
for - so no new descriptor kind and no change to `read`/`close`/`fstat`/`dup` was needed. The Win32
flag is a host-API constant (`winbase.h`), not a guest or vendor value.

## Measured: it moved the wall, and it is not the whole of it

PPSA04263 went **70 -> 71 imports (FURTHER)**: the `/app0/` open now succeeds, and only
`/host//ap/rpf.cache` is left unheld - a `/host/` devkit path that is faithfully `ENOENT` on retail
too, so the guest must tolerate it. The title still reaches an `int 0x41` at the same site, so an
upstream cause remains beyond the directory open (an assertion whose byte-at-`+0x19` check the
disassembly shows, not another missing file the report can name). That is honest: the directory bug
was one upstream wrong value the finding pointed at, it is fixed, and the trap has more behind it. The
fix stands on its own - opening a directory should never have answered "not there" - regardless of how
far this one title gets.

It is also not one title's fix: any guest that opens a directory on a Windows host was getting
`ENOENT`, and now does not.

## Made to fail

- `a_directory_opens_to_a_descriptor_not_enoent` (fs) - opening `/app0/` answers a real descriptor
  above the standard streams, not the `None` that reads as "not there". It is green on Unix before and
  after (where `File::open` always opened a directory) and is the guard that the Windows path now
  matches it - the platform where the bug lived.

## Gate state

`cargo fmt --all --check` clean, `cargo clippy --workspace --all-targets -D warnings` clean,
`cargo test --workspace` green, worklogs unique, identity scan clean. The frontier golden was
regenerated for PPSA04263's improved record (70 -> 71).
