# D930 - An event queue keeps its events as FreeBSD's kqueue keeps knotes

**Status:** decided
**Date:** 2026-10-20

`orbistoun-kernel`'s `equeue` module holds what `sceKernelCreateEqueue` makes. An event the guest
registers is a knote keyed by filter and identifier, at most one per key. It goes on the ready
list at most once, so repeated triggers before a wait come back as one event.

FreeBSD's `sys/kern/kern_event.c` is followed for each part, and the code cites the lines:

- `kqueue_register` (1665-1952) for adding, deleting, enabling and disabling an event, and for
  how `EV_FORCEONESHOT` and `EV_KEEPUDATA` change one;
- `filt_userattach` and `filt_usertouch` (1119-1204) for `NOTE_TRIGGER` and the `NOTE_FF*`
  controls;
- `kqueue_scan` (2118-2316) for delivery. A wait visits each ready entry once. An `EV_ONESHOT`
  event is dropped after delivery, `EV_CLEAR` resets one, `EV_DISPATCH` disables one, and an
  event with none of these goes to the back of the list, so every wait delivers it again.

Only `EVFILT_USER` is kept as knotes. A change naming any other filter is refused as
`EINVAL`, the answer FreeBSD gives for a filter it does not have.

Subsystem completions (flips, end-of-pipe releases) stay as they were before this decision. A
subsystem registers a route and posts finished events to it. Each completion is delivered once,
in order, on the same ready list as the knotes, and removing the route drops any completion
still waiting. Coalescing these completions would change what `data` carries for each one, and
that is not measured.

The guest entry points:

- `sceKernelAddUserEventEdge(eq, id)` adds a user event with `EV_CLEAR`. The name says edge, and
  `EV_CLEAR` (0x20) is the flags word measured on every Sony completion.
- `sceKernelTriggerUserEvent(eq, id, udata)` is a change with `NOTE_TRIGGER` that leaves the
  trigger's `udata` on the event, as `kqueue_register` replaces it without `EV_KEEPUDATA`.
- `sceKernelDeleteUserEvent(eq, id)` is `EV_DELETE`.
- `sceKernelDeleteEqueue(eq)` drops every event, and wakes a thread waiting on the queue with the
  bad-handle answer.
- `sceKernelWaitEqueue` reads its timeout as a 32-bit count of microseconds. PPSA25872 stores
  only a dword before passing the pointer (image+0x11c4208).

Entry points that no title in the library imports are not declared, following the dependency
spine: the timers, `sceKernelAddUserEvent`, and the read, write and file events. obSCEne
REQ-20261020T1315Z-kq01 measures their shapes ahead of the first title that needs them.

**Why:** the library's titles trigger user events, delete them, and delete queues
(`sceKernelDeleteEqueue` has 15 importing modules). Before this decision a user event was
registered and never delivered. It also shared one identifier space with flip ports, so a user
event whose id matched a port handle would have received flips.

**Assumed, pending kq01:** the error codes beyond ESRCH for a bad queue (an unregistered event is
`ENOENT`, a malformed change `EINVAL`); that the edge add is `EV_CLEAR`; which `udata` a
coalesced trigger leaves; and what a waiter sees when its queue is deleted.

**Rejected:**
- Keeping user events in the subsystem route list: that list cannot express coalescing, level
  events or disabling, and it shared identifiers with ports.
- Coalescing subsystem completions as knotes now: `data` on a flip completion varies from flip
  to flip in a way that is not measured.
