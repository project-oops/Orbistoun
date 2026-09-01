# D317 - "Both sides are Vulkan" was an assumption, and it happened to be true


**decided** · 2026-08-27 · a claim checked because somebody asked it to be

Asked whether the shell should render through the emulated GPU, the answer given was that it
should not - and that when frame sharing arrives, "both sides already being Vulkan makes it
easier". The first half holds. The second was **never measured**.

`eframe` is built with the `wgpu` backend, and `wgpu` selects from `Backends::PRIMARY`, which
is `VULKAN | METAL | DX12 | BROWSER_WEBGPU`. On this platform that is two candidates and
nothing in this tree pins the choice, so the claim was resting on whatever the adapter
enumeration happened to return.

Measured, on the machine this was written on:

```
orbistoun: renderer: Vulkan - NVIDIA GeForce RTX 5070 Ti
```

True here, and **true by luck rather than by design**. A different vendor, driver or wgpu
release can land on DX12, at which point sharing an image with an `ash`-based guest backend
stops being a Vulkan-to-Vulkan problem and becomes cross-API interop with worse coverage.

So the window now reports what it got, to the terminal and beside the build stamp. Same
argument as the build stamp itself: a window that cannot say what it rendered with is a
report about nothing, and this one is load-bearing for anything that later shares a surface.

**Pinning is deliberately not done yet.** Forcing Vulkan would fix the interop question and
would also refuse to start on a machine whose Vulkan driver is broken, for the sake of a
feature that does not exist. Reporting costs nothing and makes the choice visible; pinning
belongs in the change that actually needs it.

