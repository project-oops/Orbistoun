# orbistoun-gui

The desktop window.

**Models:** nothing. It reads state and draws it.

**Deliberately fakes:** nothing.

**Design note.** Principle 13: the crates are the emulator, and `orbistoun-cli`, this, and
worker mode are interaction shims over them. Every decision - what a title is, what a
container contains, whether a run got further - is made below this and is reachable from
the CLI too.

**Writing it is what proved the rule.** Three things the CLI had quietly absorbed came out
within an hour of starting: the run comparison, the previous-trace load, and worker
bootstrap. None looked like logic in a shim until a second shim needed them (D160).

**Immediate mode, deliberately.** A call tail, a register dump and an import ranking are
tables that change wholesale every time a run finishes. Immediate mode draws from current
state each frame, which is exactly that shape; a retained widget tree would need syncing
against state that is replaced rather than edited (D161).

**Capture, and what it is not.** The toolbar writes the window to a PNG under
`<data>/screenshots/`. **Not a guest frame** - there is not one yet - and the control is
labelled *capture* rather than *screenshot* so it does not borrow a meaning it cannot
honour. Recording sits beside it, disabled, with the reason on hover: a control that
vanishes reads as a bug, a greyed one reads as a state (D215).

**Status:** runs. Almost everything it could assert about is asserted below it, and what is
left is drawing - which is the intended shape, not an omission, and does mean most changes
here are only checked by looking at them.

The exception is `capture.rs`, and it is worth naming as one. Encoding a frame and turning
a guest's own metadata into a filename are not drawing: they can be wrong in ways nobody
sees until a directory has an unopenable file in it. So they are tested, including the case
that matters most - a frame with no pixels is refused rather than written, because a
zero-byte PNG is the failure that looks like success from the toolbar.
