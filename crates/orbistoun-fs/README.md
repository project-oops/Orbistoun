# orbistoun-fs

Guest filesystem - libkernel file IO and the async streaming layer.

**Models:** the POSIX-shaped libkernel file calls - open, close, read, write, and seek -
against a sandboxed host directory, plus declarations for the rest.

**Deliberately fakes:** the async streaming layer entirely, and everything beyond the
five synchronous calls.

**Design note.** Two layers, one job. The libkernel calls are POSIX-shaped and map
almost directly onto host IO; the async streaming layer is the vendor's own, and it is what
open-world titles actually use.

**Path sandboxing is not optional.** Guest paths (`/app0/..`, `/savedata0/..`) are
mount points, and every one must resolve inside a directory orbistoun owns. A guest
path that escapes to a host path is a straightforward arbitrary-write vulnerability,
so translation goes through one function with one test suite rather than being
open-coded per call site.

**Status:** five functions implemented. One title reads ten texture files through them
with correct sizes, which is what confirms the layer rather than inferring it. The async
layer is unscheduled - not reachable until threading works.
