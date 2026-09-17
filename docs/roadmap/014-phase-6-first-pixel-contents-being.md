# Phase 6 - First pixel *(contents being built ahead of it)*


`orbistoun-gpu` + `orbistoun-gpu-vulkan` + `orbistoun-video`, and the GUI's output
surface. A Vulkan device, swapchain, and enough command translation to service one
flip.

This is also where D032's deferred cost comes due: output is produced in the worker
while the window lives in the shim. **Settled by D695, and as neither of the two
options D032 named**: the worker owns no window and no surface, renders headless,
reads the frame back to ordinary bytes, and the shim uploads those as a texture -
the path `framebuffer.rs` already takes and `egui` already displays.

That matters for the order of work here rather than only for the answer. The worry
was that shared images would constrain device creation, queue ownership and image
allocation from the first line, so it had to be decided before a renderer existed.
It constrains none of them, so the device and swapchain work below can start without
carrying it.

**Observable result:** a window with something in it. Also the arrival of framebuffer
diffing, the only cheap mechanical correctness oracle this project will ever have
(see [TESTING.md](../TESTING.md)).

