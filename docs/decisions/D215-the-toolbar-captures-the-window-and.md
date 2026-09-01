# D215 - The toolbar captures the window, and says that is what it captures


**decided** · 2026-08-24 · asked for directly

A screenshot button and a recording button on the emulator toolbar, with recording
disabled for now.

### What "screenshot" honestly means here

In every other emulator it means the guest frame. **There is no guest frame.** No title in
the corpus reaches its own main loop, nothing has been submitted to a command buffer, and
`orbistoun-video` has no output surface. So the button cannot mean what the word usually
means, and shipping it labelled `screenshot` would borrow a meaning it cannot honour -
which is principle 3 applied to a label rather than a return value.

It is labelled **capture**, its hover says *write this window to a PNG*, and the module doc
says the same at more length. What it takes is still worth having: this window is a dense
diagnostic surface - a call tail, a register dump, a ranked finding list - and "paste the
panel that says this" otherwise means an operating-system screen grab.

When phase 6 lands and there is a guest frame, that is the seam it arrives at. The
composition changes; the encoding, the naming and the destination do not.

### Recording is disabled, not absent

It needs a frame source and an encoder and has neither. Disabled with the reason on hover,
because that is this toolbar's rule throughout: *a control that vanishes reads as a bug, a
greyed one reads as a state*. Somebody will look for recording; finding it greyed with an
explanation is a better answer than finding nothing and wondering.

No encoder dependency was added. Adding one to serve a button that cannot work yet is the
shortcut principle 11 exists to refuse.

### Two halves, because a window cannot read its own pixels

Asking egui for the frame is a viewport command and the answer arrives as an input event on
a **later** frame. So the request is issued after composition - so what comes back is the
window as just drawn - and the reply is collected at the top of the next update, before the
toolbar draws, so the outcome is reported in the same frame it arrives.

The outcome is shown in the toolbar rather than logged. A file written where the user
cannot see it is the same to them as no file; a failure that reaches only a log is worse,
because the button looked like it worked.

### It also removed a duplicated list, which is why it took longer than it looks

`Paths` had `all_dirs()`, and `orbistoun-cli paths` had a second hand-written enumeration
of the same directories. So a new writable location could be added, pass the containment
test that walks `all_dirs()`, and **still never appear in the answer to "where did it
go?"** - which is the entire question that command exists for.

Exactly the shape catalogued in D213: one rule, written twice, drifting quietly. `Paths`
now exposes `named_dirs()` as the single list and both read from it. The column width is
measured from it too, rather than typed, because a ten-wide column was correct right up
until `screenshots` was eleven characters long.

That was not the task. It was two lines of the task and the rest of it was the reason the
task would have gone wrong.

