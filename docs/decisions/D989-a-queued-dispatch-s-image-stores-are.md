# D989 - A queued dispatch's image stores are bounded by their descriptors

**Status:** decided
**Date:** 2026-10-25
**known_by:** measured

Amends D876 and D982. A queued compute dispatch that stores to an image, through a flat address
that cannot reach guest memory, or device-addressed through buffers the host resolves, is no longer
queued as writing everything; and an indirect dispatch's writes are queued at all. Its queued writes are
worked out as D876 works out its buffers', before it runs, without translating it.

**Images.** A storage image's whole range is known from its descriptor. When a dispatch is
prepared, the pipeline keeps where its translation found each stored image's descriptor (the
module's stored `TextureSource`s), keyed by the program's address and a hash of its bytes. The
queued writes read each descriptor from the dispatch's user data or table, as the preparation
does, and add:

- the level the view names, where the descriptor places it (`dispatch_image_surface`): all the
  write-back of a stored image reaches;
- under compression, the whole chain, which cleared keys expand to zeros, and the keys, which the
  write-back marks uncompressed;
- nothing for a depth target or its stencil plane the host holds (D894), whose store is written
  back to the host.

A program no translation has named yet - its first dispatch - is still unbounded, and so is a
descriptor that does not place the image. Those are counted as `unbounded: image store`.

**Flat stores.** A flat store whose address's high word is at least `0x8000` reaches no guest
memory. The guest runs in the host's user address space, which ends below 2^47, so no guest
mapping lies there; the store reaches the local data share or the private aperture (D964), or
nothing. `draw_buffers::flat_stores_beyond_guest` proves the bound for a store whose high word a
`v_med3_u32` set in the same straight run, after the last change of the execution mask, with two
operands that are constants or scalar registers the trace holds as constants: a median is at least
the smaller of any two of its operands. Grand Theft Auto V's ray-traversal passes push their stack
this way, the high word clamped between `0x70000000` and `0x80000000`. Any other flat store is
unbounded, counted as `unbounded: flat store`.

The flat-encoded family's mnemonics name no segment, so a flat store is told by its `SEG` field
(`draw_buffers::flat_segment_store`). A store in the scratch segment is the lane's own private
memory and writes no guest memory. A flat store not known to stay off guest memory is taken as a
global one: bounded where the trace serves it (D961), else unbounded, counted as
`unbounded: flat store`.

**Device-addressed dispatches.** Where the host cannot trace a dispatch and the backend offers
device addressing (D982), the queued writes trace it as its preparation will, device-addressed: the
buffers the host resolves are still bound (D964), so a store through one writes that buffer alone.
Only a store through a buffer the host does not resolve, or an untraced global or flat store,
reaches guest memory by device address, and leaves the dispatch unbounded, counted as
`unbounded: device-addressed`. A program that the command processor prepares with host runs instead
(D982) binds a superset of these buffers, so the bound still holds.

**Indirect dispatches.** A `DISPATCH_INDIRECT` was left out of the queued writes altogether: only
`DISPATCH_DIRECT` packets gave a dispatch state, so what an indirect dispatch wrote was read by
later preparations as it stood before. Its state is now recorded too
(`Submission::indirect_dispatch_states`), one group standing in for the grid the command processor
reads from memory as it carries it out. Its writes are bounded where the grid does not decide them
(`Pipeline::indirect_dispatch_write_spans`): a buffer a descriptor names, and an image. A store
through the window, the global buffer's pages or a buffer placed from a sample of the waves, or a
trace that would run the groups on the host, leaves it unbounded, counted as
`unbounded: indirect grid`. Grand Theft Auto V dispatches six programs indirectly each cycle
(`0x74005b750e00` and its neighbours), most of them with a grid of no groups.

**What stays unbounded.** A device-addressed store through a buffer the host does not resolve, an
untraced global store, an image store before the program's first preparation, and a draw that
streams out. The first time a program is unbounded for a reason, the log names it and where.

**Why:** in Grand Theft Auto V's world about 17 of 88 queued submissions a flip had no write
bound, all for an image or a flat store, and every later preparation waited for each of them. The
technique is the usual one for tracking device writes ahead of a run: each written binding's guest
range, from its descriptor, a storage image's from the surface the descriptor describes and its
metadata, and no bound at all where the shader addresses memory directly.

**Tests:** `a_stored_image_writes_the_level_its_descriptor_places`,
`a_queued_image_store_is_bounded_once_its_program_is_translated`,
`a_flat_store_clamped_to_the_apertures_reaches_no_guest_memory`,
`a_device_addressed_dispatch_writes_the_buffers_it_resolves`,
`an_indirect_dispatch_s_state_is_recorded`.

**Rejected:**
- Bounding a device-addressed dispatch by the mapped runs its table reaches: those are every
  readable guest mapping, which every preparation read overlaps, so it bounds nothing.
- Bounding it by the chunks an earlier dispatch of the program marked written (D974): that predicts
  rather than bounds, and a store to a chunk the earlier one missed would be read stale.
- Translating each queued dispatch to learn its images: the translation is what the command
  processor's preparation pays for, and the queued writes are worked out on the guest's thread.
