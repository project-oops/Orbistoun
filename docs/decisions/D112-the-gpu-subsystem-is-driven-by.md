# D112 - The GPU subsystem is driven by submissions, not called by the emulator


**Status:** decided (2026-08-21) - and the read window's failure path is pinned

`orbistoun-gpu::pipeline` takes a submitted command buffer and a reader for guest memory
and produces backend commands. Nothing above it chooses a shader, an address, or a
stage; everything is derived from what the guest wrote.

That is not a stylistic preference, it is what this generation of hardware forces. The
guest builds command buffers and talks to the hardware, so there is no high-level
graphics call to intercept. A translator waiting to be handed a shader would wait
forever.

**This is the seam that was missing.** Every piece already existed - packet walking,
register decoding, shader translation, a backend command vocabulary - and none of them
touched. A shader translator nothing calls is a library rather than a subsystem, and it
cannot be wrong in any way a test would notice, because the only shaders it ever sees
are the ones its own tests hand it.

**Guest memory is a one-method trait.** A shader lives at a guest virtual address, so
this must read guest memory and must not depend on the address space to do it. That
keeps the crate testable with a `Vec<u8>` and leaves the address space free to change.

**A shader that will not translate is reported, never skipped.** It does not become a
no-op and it does not stop the submission: the binding command is omitted and the failure
is recorded with its address and reason. A frame missing a draw is visible; a frame where
a draw silently drew nothing is a week of somebody's life. The report is also the shape a
worklist wants, which is the same argument the import survey made one layer up.

### The 64 KiB read window, and why its size is the less interesting half

A shader has no declared length, so it is read as a window and decoded until an
end-of-program instruction. The window was picked at 64 KiB with nothing to check it
against, and that gap sat open until there were compiled shaders to measure.

There are now, and the largest is **320 bytes** - the window is generous by two orders of
magnitude. That is reassuring and it is not the property worth pinning, because the number
will stop being generous the first time something big arrives and no test would notice.

What matters is what happens when a shader *doesn't* fit, and that was untested. Truncating
would be the bad outcome and the plausible one: a real shader cut at 64 KiB decodes
cleanly right up to the cut and produces a module that is a genuine prefix of the correct
one. Nothing about it looks wrong. So the window running out is a **refusal**, reported
with the address and a reason naming the end-of-program instruction it never found - a
fact somebody can act on by raising the window, rather than a frame that renders
almost-right.

`a_shader_with_no_terminator_in_the_window_is_refused_rather_than_truncated` holds that,
and also holds that the address is still reported as resolved: a window that ran out says
nothing about whether the address was right, and conflating the two would give D101 a
false signal.

