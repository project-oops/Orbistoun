# orbistoun-gui

The desktop window. It reads state and draws it, and models nothing of its own.

`orbistoun-cli`, this window and worker mode are interaction shims over the crates
(CLAUDE.md, *Shims hold no logic*). Every decision - what a title is, what a container
contains, whether a run got further - is made below this crate, mostly in
[orbistoun-service](../orbistoun-service/), and is reachable from the CLI too. A run is driven
through [orbistoun-worker](../orbistoun-worker/); the shell model is
[orbistoun-shell](../orbistoun-shell/) and the controller model is
[orbistoun-input](../orbistoun-input/).

## Immediate mode

The window is immediate-mode (egui). A call tail, a register dump and an import ranking are
tables replaced wholesale every time a run finishes; immediate mode draws from current state
each frame, where a retained widget tree would need syncing against state that is replaced
rather than edited.

## Capture

The toolbar's capture writes the window to a PNG under `<data>/screenshots/`. The control is
labelled *capture*, not *screenshot*, because what it records is the emulator window. A
control that cannot be used is shown disabled with the reason on hover rather than hidden: a
control that vanishes reads as a bug, a greyed one reads as a state.

## Tests

Almost everything the window could assert is asserted in the crates below it, so most
changes here are checked by looking at them. `capture.rs` is the exception and is tested:
encoding a frame and turning a guest's own metadata into a filename can fail in ways nobody
sees until a directory holds an unopenable file. A frame with no pixels is refused rather than
written, because a zero-byte PNG is the failure that looks like success.
