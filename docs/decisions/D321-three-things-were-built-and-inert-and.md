# D321 - Three things were built and inert, and one of them could not be built at all


**decided** · 2026-08-27 · found by asking what the shell actually does rather than what it models

The shell had a lifecycle, an event queue, settings, a control channel and a tile wall, and
three pieces of it did nothing. Each looked finished, which is the point worth recording.

**Settings could not be set.** `shell.toml` was read, `user_name` was drawn, and nothing
edited or saved it. The argument for building a shell at all was that a console setting is a
fact about what the owner wants and the owner is right there - and the owner had no way to
say. Now a pane edits all three and both files are written by one button.

That pane **bends this module's own rule**, which says every control takes effect on the next
run. Two of the three do not reach a guest, because no encoding has been measured. Hiding
them means nothing can ever be set; showing them silently is the dropdown-over-nothing
failure the rule exists to prevent. So the pane states which is which, in terms of something
checkable - *"0 parameters have a measured encoding"* - counted rather than asserted, so it
cannot drift out of date and needs no editing when it stops being zero.

**Quit told nobody.** `Request::Quit` moved the session to `Exited` and raised `Quitting`,
and the `Stopper` that actually ends the worker was never wired to it. A `Stopper` is
`TerminateProcess` - on its own that is pulling the power out. `WorkerHandle::control()` now
mirrors `stopper()`: taken before the handle is moved into the thread that blocks on it, and
carrying a shell action *in* rather than a kill. The order is action-then-terminate, and it
becomes correct the day a code is measured without anything here changing.

**Focus could not be wired, and the reason is that there is nothing to wire it to.**
`Focus::neutral_for_title` exists and is tested; `scePadReadState` does not exist.
`orbistoun-input` is twenty-seven lines - a `guest_module!` declaration and nothing else. The
preferences window already said so and I proposed the work anyway, having read the
declaration and taken it for an implementation. Gating a pad shim behind focus is a real
piece of work that begins with writing the pad shim, and principle 6 says that is out of
order while no guest reaches it.

**The pattern across all three.** Every one was modelled correctly and connected to nothing,
and each read as complete from its own file. A type that describes a behaviour is not the
behaviour, and this project has now written that down twice in one day - once about events
that were never delivered, once about the buttons that would have delivered them.


