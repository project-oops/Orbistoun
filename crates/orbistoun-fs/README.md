# orbistoun-fs

The guest filesystem: libkernel file IO, descriptors and sockets, and the vendor async
streaming layer.

The crate covers two layers. The libkernel calls are POSIX-shaped and map almost directly
onto host IO: open, read, write, seek, directory entries, metadata, `fcntl`, BSD sockets,
`select` and `kqueue`. Above them sits the vendor async streaming layer, which is what
open-world titles use. [orbistoun-libc](../orbistoun-libc/),
[orbistoun-posix](../orbistoun-posix/) and orbistoun-net build on its descriptors.

## Main pieces

| Module | Holds |
|---|---|
| `mount` | the mount table: where a guest path lands on the host |
| `filesystem` | the base tree materialised from a knowledge file, with a per-title writable overlay |
| `sandbox` | a title's sandbox, assembled in one fixed order: empty the overlay, install the base tree, layer the title's files over `/app0` |
| `descriptor` | file descriptors and the standard streams |
| `socket`, `select`, `kqueue` | BSD sockets mapped onto the host's, and readiness waiting |

## Rules

- **Path sandboxing.** Guest paths (`/app0/...`, `/savedata0/...`) are mount points, and every
  one resolves inside a directory orbistoun owns. A guest path that escapes to a host path is
  an arbitrary-write vulnerability, so translation goes through one function with one test
  suite rather than being open-coded per call site.
- The mount table takes no special case for a particular title; per-title behaviour belongs
  in [orbistoun-overrides](../orbistoun-overrides/).
- A guest's standard output never goes to the worker's standard output, which carries the
  worker protocol.
