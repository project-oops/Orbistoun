# D964 - Ray traversal passes address guest memory directly

**Status:** decided
**Date:** 2026-10-25
**known_by:** guest-observed

A compute dispatch whose memory ranges the host cannot know before it runs is run
device-addressed instead of being refused. Every guest mapping is mapped whole into the device's
address space, and the translated module turns each guest address it forms into a device address
and reaches the guest's bytes where they lie. This relaxes D900 for such dispatches: the host does
not run their waves to find what they reach.

**Mapping.** Option A: each reservation's readable runs, the pages the guest last left readable,
are mapped whole (`VK_EXT_external_memory_host`, `bufferDeviceAddress`). Each run is committed
first, because the driver touches every page as it maps it. The write-watch's read-only guards over
it are released then too, its pages noted written (D824). Only the pieces of a run whose host pages
are then committed, private and read-write are mapped; a page the guest may not write is left out,
and what reaches it is an escape. Run 335 of Grand
Theft Auto V gave the driver a run whole, and the driver faulted inside `vkAllocateMemory`; how
the import treats any other page is the driver's, and not promised. `orbistoun-mem` keeps the runs and a
generation that moves when they change (`mappings`). The backend maps again only when the
generation has moved, and drops a mapping before its reservation's host memory is released.
Amended by D968: a piece left out is looked at again whenever a host page is committed or given
another protection, and what has become committed and read-write in it is mapped. The
release waits for any dispatch still reading it. A table keyed by chunk turns a guest address into
a device address. A chunk is at most 64 KiB and never coarser than any mapping's alignment, so one
chunk lies in one mapping. The slots form a bounded-probe hash table, as D961's pages do. The push
block names the table at `DEVICE_TABLE_PUSH_WORD`. An address no mapping holds reads zero and stores
nothing, and is noted as an escape. Amended by D969: the table lies in device-local memory where
the device has room for it, and its probes are consecutive sixteen-byte slots. The lookup is one
helper function every access calls, and an access's words share two lookups.

**What goes by device address.** The dispatch's global and flat accesses, and its scalar and
untyped buffer accesses whose descriptor or base the host could not resolve without running the
program (D753, D758). Buffers the host does resolve stay bound as before (D746), so a format access
still converts by the format the host read from its descriptor. A format access through a
descriptor the host cannot resolve is still refused. Amended by D967: a format load through one
converts by the format the descriptor names, decoded as the module runs, and a descriptor's
swizzled and lane-indexed addressing is carried out rather than refused. A ray intersection reads its node by device
address. Amended by D970: a scalar load of an image descriptor from a table is read by device
address too. The image still binds from the load that reaches it, and the program may read the
words as values.

**Apertures.** A flat access routes by its address's high half before it reaches guest memory:
`0x80000000` reaches the workgroup's local data share, and `0x70000000` reaches the lane's private
memory as a scratch-segment access names it (D954). Any other value reaches guest memory. The three
programs keep their traversal stacks this way. They clamp a stack pointer's high half between those
two values, and read the entries back with `ds_read_b32` and `scratch_load_dword` respectively.

**Caches.** A device store passes the host's write-watch and page protection by. Each store
therefore marks its chunk's slot. Amended by D969: the marks lie apart from the table, in host
memory, one word for each slot. After the dispatch, the backend hands the marked
chunks to the command processor. The command processor treats each chunk as a host write: a
protected target's trust is dropped (D720), and the write-watch notes the chunk's pages written
(D824). Stale textures and draw buffers over them are therefore not reused. Before the dispatch, the
frame pending on the device and every deferred copy are written into guest memory, because the
dispatch may read anywhere.

**Known limit.** A buffer the host resolved is still bound and written back after the dispatch
(D746). Where the dispatch also stores to the same bytes by device address, the write-back lands
last. No program met so far does both.

**Why:** Grand Theft Auto V's three full-screen ray-traversal passes, the compute programs at
`0x74005b854a00`, `0x74005b857200` and `0x74005b7e8600`, walk a two-level hierarchy. The
instance a ray enters names, through `s_load_dwordx16` from an address it computes, the bottom
level's base. That base forms the descriptor of the next `image_bvh_intersect_ray` and of a
`buffer_load_dwordx2`. What they read depends on the intersections, so the host cannot work it
out. Their preparation was refused under D758 (a branch on an execution mask the host cannot
know) and D753 (a descriptor computed from what the host cannot know), and the passes were
skipped.

**Rejected:**
- Conservative page binding, which binds every page a descriptor table could name: the hierarchies
  span gigabytes, and what a ray reaches is not known before it runs.
- Emulating ray intersection on the host to find the nodes: running the whole traversal on the CPU
  per frame, which is the dispatch itself.
- Mapping only committed pages: a page the guest CPU never touched is uncommitted, and a device
  store there would be lost or fault. Runs are committed whole before they are mapped instead.
