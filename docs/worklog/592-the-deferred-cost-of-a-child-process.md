# 592. The deferred cost of a child process comes due, and is cheaper than it was booked at

**2026-09-15** - step 3 of the gap analysis: the worker/window model, decided

## The question

D032 put guest code in a child process for an address-space reason that still stands, and booked a
cost against phase 6: *"output is produced in the worker while the window lives in the shim, so it
needs either a reparented child-owned window or shared images via external-memory extensions."*

The gap analysis put this third in its sequence and gave the right reason for the position: **if
the answer were shared images, it would constrain device creation, queue ownership and image
allocation from the first line of the renderer**, and retrofitting it later would be a rewrite of
whatever was built first. So it had to be answered before gap 1, not during it.

## The answer

Neither of the two options D032 named. **The worker owns no window and no surface; it renders
headless, reads the frame back to ordinary bytes, and the shim uploads those bytes as a texture.**
D695.

And the load-bearing part for the sequence: **it constrains none of the three.** The renderer can
be built now without carrying a constraint it turns out not to need.

## What decided it, all of it read rather than assumed

- **No swapchain exists anywhere.** `orbistoun-gpu-vulkan` depends on `ash`, `orbistoun-gpu` and
  `thiserror` - no surface extension, no windowing crate. The two `swapchain` hits in the
  workspace are the *guest's* flip queue in `orbistoun-video`, which is a different thing wearing
  the same word.
- **The proven path is already headless with readback**: `framebuffer.rs` renders to a storage
  image, copies to a host-visible buffer, and produces `Pixels { width, height, bytes }`, tested
  on a real device.
- **The shim already displays CPU pixels**: `orbistoun-gui` is `eframe`/`egui` and already builds
  `egui::ColorImage` and `TextureHandle`.
- **Bulk already crosses the process boundary by file, not by pipe**: the worker writes its trace
  to `traces_dir` and the shim reads it there, while the pipe carries only control events. A frame
  is bulk, so it takes the route bulk already takes.

The argument that settles it is short: **framebuffer diffing is the only cheap mechanical
correctness oracle this project will ever have, and it compares pixels on the CPU.** A readback
path to ordinary bytes must exist under every option. Choosing it as *the* path adds no second
path; choosing either alternative adds one and keeps it forever.

## Recorded against each rejected option

- **A reparented child-owned window** is platform-specific in the worst way (Wayland has no
  reparenting at all), couples the worker to a windowing stack, forecloses the headless path that
  CLI runs and sweeps use, and collides with D032's *own* third argument - the dev loop restarts
  the worker constantly, so the window would be destroyed and recreated every iteration.
- **Shared images via external memory** needs both processes on the same physical device with
  matching UUIDs, and the shim's Vulkan device lives inside `eframe`'s renderer backend, so
  importing means reaching through `egui` into whichever backend it built with. It is the option
  that carries the constraint, and it still needs the readback for diffing.

## The cost, stated rather than discovered

A copy and a transfer per frame: 8.3 MB at 1080p `R8G8B8A8`, so a sustained 60 Hz would be about
half a gigabyte a second, which this route will not carry well. Written into D695 as a known
ceiling rather than left to be found.

When it binds, external memory becomes an optimisation **behind the same seam** - the worker
produces a frame, the shim displays it - because nothing above the seam knows how the bytes
arrived.

## Two documents corrected, for the reason worklog 589 exists

Both still posed this as open, and a document that outlives its question is the same defect as the
one that said *"nothing here is implemented"* over sixteen handlers:

- `docs/roadmap/014-phase-6-first-pixel...` now records the answer and, more usefully for whoever
  picks up phase 6, that the device work below it is unconstrained.
- `D032` carries a closing pointer. Its cost paragraph stands as written - it is an accurate record
  of what was understood when the process boundary was chosen, and rewriting history there would
  lose that.

## Surprise

**The gap analysis attributed D032's sentence to the roadmap.** It reads *"ROADMAP phase 6 records
the cost"* and then quotes it - and phase 6 does carry those words, because phase 6 is quoting
D032. Harmless here, since both say the same thing and the substance checked out, but it is the
second time in two days that a document's claim turned out to be a quotation of another document
rather than an independent statement. Two sources agreeing is worth much less when one of them is
repeating the other.

**And nothing was built.** This unit produced no code at all, which is the correct output: the
whole value was establishing that the constraint everyone was budgeting for does not exist, so the
next unit can start without it.
