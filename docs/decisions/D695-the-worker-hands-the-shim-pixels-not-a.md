# D695 - The worker hands the shim pixels, not a window and not GPU memory

**assumed** - 2026-09-15

D032 put guest code in a child process and named the cost it deferred: *"output is produced in the
worker while the window lives in the shim, so it needs either a reparented child-owned window or
shared images via external-memory extensions."* Phase 6 quotes it. This settles that, and it
settles it as the third answer rather than either of the two named:

**The worker owns no window and no surface. It renders headless, reads the frame back to ordinary
bytes, and the shim uploads those bytes as a texture.**

D032 itself is untouched - the child process stays, for the address-space reason that decided it.

## Why this was worth deciding before a renderer exists

Because one of the two named options would have constrained the renderer from its first line.
External-memory sharing dictates how the device is created, which queue owns what, and how images
are allocated; choosing it late means rewriting whatever was built first. So the question had to
be answered before gap 1, not during it.

**The answer is that it constrains none of them**, and that is the substance of this entry: the
renderer can now be built without waiting, and without carrying a constraint it turned out not to
need.

## What the tree already does, which decided it

Read rather than assumed:

- **There is no swapchain anywhere.** `orbistoun-gpu-vulkan` depends on `ash`, `orbistoun-gpu` and
  `thiserror` - no surface extension, no windowing crate. The two `swapchain` hits in the
  workspace are the *guest's* swapchain in `orbistoun-video`, which is the flip queue the guest
  talks to, not a host presentation chain.
- **The proven path is headless with readback.** `framebuffer.rs` renders into a storage image and
  copies it to a host-visible buffer, producing `Pixels { width, height, bytes }` - tightly packed
  `R8G8B8A8_UNORM`. Tests exercise it on a real device.
- **The shim already displays CPU pixels.** `orbistoun-gui` is `eframe`/`egui` and already builds
  `egui::ColorImage` and `TextureHandle` (`capture.rs`, `icons.rs`). A frame is its ordinary path,
  not a new one.
- **Bulk already crosses the boundary by file, not by pipe.** The worker writes its trace to
  `traces_dir`; the shim reads it there. The pipe carries control and small events - `Hello`,
  `Reached`, `SurveyComplete`, `Terminated`, `Failed` - and no payloads.

## The argument that settles it

**Framebuffer diffing is the only cheap mechanical correctness oracle this project will ever
have** (CLAUDE.md's oracle list, TESTING.md, and phase 6's own entry). It compares pixels
numerically, on the CPU. So a readback path to ordinary bytes has to exist **whatever** is chosen
for display.

That makes the comparison lopsided. Choosing readback as *the* path adds no second path. Choosing
either of the others adds one, and keeps it forever, for frames that still have to be read back to
be checked.

## The two named options, and why not

**A reparented child-owned window.** The worker would create an OS window and the shim would adopt
it. Rejected on four counts: it is platform-specific in the worst way (Win32 reparenting, X11
reparenting, and Wayland which has no reparenting at all); it couples the worker to a windowing
stack it otherwise has no reason to link; it forecloses the headless path that CLI runs, sweeps
and CI all use; and it fights D032's own third argument - *the dev loop is load → crash → tweak →
reload*, so the window would be destroyed and recreated on every iteration. `egui` could not draw
over a foreign native window either, so the inspector and the frame could not share a surface.

**Shared images via external memory.** The worker would export a `VkImage` and the shim import it.
Rejected for now on cost and reach: both processes need Vulkan devices on the *same* physical
device with matching UUIDs, and the shim's device is inside `eframe`'s renderer backend - so
importing means reaching through egui into whichever backend it built with, which is
backend-dependent surgery. It is the option that constrains device creation, queue ownership and
image allocation from line one. And it still needs the readback path for diffing.

It is the right optimisation *later*, which is the next section.

## What this decides, precisely

- The worker renders headless. No surface, no swapchain, no window.
- A frame crosses as **bytes in a shared region named by a small descriptor** on the existing
  protocol - dimensions, format, sequence - and never as pixels inside a JSON message. That
  follows the route bulk already takes, and keeps D035's rule that protocol messages carry no
  handles and no references.
- The shim uploads those bytes as a texture and draws it like any other.

## What it deliberately does not decide

- **Whether the GPU renders into the guest's registered buffer or into an image of its own and
  copies.** That is a question for the renderer and the flip queue, and the answer here holds
  either way.
- **The exact bulk mechanism** - memory-mapped file, shared memory, or something else. D035 makes
  the transport swappable on purpose, and the first implementation should be the simplest thing
  that works. Naming one here would be inventing a design nobody has measured a need for.

## Cost, stated honestly

A copy and a transfer per frame. At 1080p `R8G8B8A8` that is 8.3 MB a frame, so a sustained 60 Hz
would be about half a gigabyte a second - which this route will not carry well. **That is a real
ceiling and it is stated rather than discovered.**

It is also the right ceiling to accept now. Correctness comes before frame rate in every ordering
this project has written down, the first frame is a correctness result and not a performance one,
and nothing in the corpus has produced a pixel yet.

When the ceiling binds, external memory becomes an **optimisation behind the same seam** - *the
worker produces a frame, the shim displays it* - rather than a rewrite, because nothing above the
seam knows how the bytes arrived, and because choosing this constrained neither the device, the
queue, nor the image allocation.

## What would overturn this

- Sustained frame rate at resolution becoming the goal before correctness is established. Then the
  export path is worth building, and everything it needs will already exist.
- A shim that cannot upload a texture cheaply. Not the case for `egui`, and a second shim would
  have to be very unusual for it to be the case there.
