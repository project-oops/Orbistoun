# Phase 6 - First pixel *(contents being built ahead of it)*


`orbistoun-gpu` + `orbistoun-gpu-vulkan` + `orbistoun-video`, and the GUI's output
surface. A Vulkan device, swapchain, and enough command translation to service one
flip.

This is also where D032's deferred cost comes due: output is produced in the worker
while the window lives in the shim, so it needs either a reparented child-owned
window or shared images via external-memory extensions. Deferred deliberately - until
now the worker produces no video at all.

**Observable result:** a window with something in it. Also the arrival of framebuffer
diffing, the only cheap mechanical correctness oracle this project will ever have
(see [TESTING.md](../TESTING.md)).

