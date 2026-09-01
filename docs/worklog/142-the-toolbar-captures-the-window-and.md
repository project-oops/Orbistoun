# 2026-08-24 - The toolbar captures the window, and recording is greyed out (D215)


**Done.** A capture button and a disabled record button on the GUI toolbar, PNGs under
`<data>/screenshots/`, three tests.

### The only real decision was what to call it

In every other emulator "screenshot" means the guest frame. **There is no guest frame** -
nothing reaches a main loop, nothing has been submitted, `orbistoun-video` has no output
surface. So the word would have been borrowed for something it cannot deliver.

It is **capture**, the hover says *write this window to a PNG*, and the module doc says so
at length. What it takes is still worth having: this window is a call tail, a register dump
and a ranked finding list, and "paste the panel that says this" otherwise means an
operating-system screen grab. When phase 6 arrives, the composition changes and the
encoding, naming and destination do not.

Recording is disabled with its reason on hover rather than absent, which is this toolbar's
existing rule - a control that vanishes reads as a bug, a greyed one reads as a state. **No
encoder dependency was added** for a button that cannot work yet.

### Verified rather than assumed

`ViewportCommand::Screenshot` is honoured by the **wgpu** backend, not only glow - checked
in `eframe-0.29.1/src/native/wgpu_integration.rs` before relying on it. A button that
silently does nothing is precisely the failure this project keeps writing decisions about.

### The part that was not the task

`Paths::all_dirs()` and `orbistoun-cli paths` were **two hand-written lists of the same
directories**. A new writable location could be added, pass the containment test that walks
`all_dirs()`, and never appear in the answer to *"where did it go?"* - which is the entire
reason that command exists. Adding `screenshots` would have landed in that gap.

Same shape as everything in D213. `named_dirs()` is the one list now and both read from it.
The column width is measured from it rather than typed, because ten was correct until
`screenshots` was eleven characters.

The containment test caught the change itself, which is what it is for: *"a location was
added without updating the test"*.

### Tested, in a crate whose README says it has no tests

That claim was right and is now qualified. Encoding a frame and turning a guest's own
metadata into a filename are not drawing - they fail in ways nobody sees until a directory
holds a file that will not open. So: a real PNG written and read back by signature, a title
containing `:` `/` `?` reduced to something Windows accepts, and **a zero-pixel frame
refused rather than written**, that last being the failure that looks like success from the
toolbar.

