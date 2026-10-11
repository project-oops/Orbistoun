# D792 - A save is mounted from the title's own storage

**Status:** decided
**Date:** 2026-10-11

`sceSaveDataMount3` mounts the save its parameters name at `/savedata0`. The save is the directory
of that name under the title's writable `/data/savedata`, so it lives in the title's overlay and
persists as everything else the title writes does (`ORBISTOUN_SANDBOX=ephemeral` empties it).
A mode with bit 2 (create) makes a missing save; without it a missing save answers `0x809f0008`.
The mount is writable only when the mode has bit 1 (read-write). The result takes the mount point
at +0x00, zero required blocks at +0x10 and 1 at +0x1c when the save was created.
`sceSaveDataUmount2` releases the mount.

Assumed (`known_by = assumed`): the mode bits, the result layout and the not-found code are the
SDK's as public headers give them, used as an oracle. The plugin's own reads confirm only where it
looks (`result+0x10` u64, `+0x1c` u32). obSCEne could not bind `Mount3` from a homebrew process
(REQ-sm02), so a measurement replaces these when one exists.

**Why:** the user chose it on 2026-10-11 over answering "no save data" and over waiting. PPSA03416's
save plugin mounts at start-up and does not go on without it.

**Rejected:**
- Answering not-found always: the title would try to create and fail the same way.
- A save store outside the title's overlay: a second place to clear and back up.
