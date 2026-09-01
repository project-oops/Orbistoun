# D318 - The overlay is blocked earlier than "cross-process presentation"


**decided** · 2026-08-27 · two claims checked and both found wrong

The composition problem was described as: the title's frame comes out of the emulated GPU,
the shell's UI comes out of wgpu, and the two must be composited - blocked on getting pixels
between processes. Checking it found the description wrong in two places.

**There is no guest frame.** `orbistoun-gpu-vulkan` is `compute.rs` and `lib.rs` - a compute
path for dispatching translated shaders. There is no swapchain, no graphics pipeline and no
image, and `present()` returns `BackendError::Unsupported` with a test named
`present_is_refused_too` asserting exactly that. Nothing renders a frame to share.

**Flip status is declared, not implemented.** `sceVideoOutSubmitFlip` and
`sceVideoOutGetFlipStatus` appear in the `guest_module!` declaration - so they are named in a
trace - and `implementations()` carries only `Open`, `Close` and the two `RegisterBuffers`.
The argument that a compositor changes what `GetFlipStatus` must report is sound *and*
premature: it reasons about behaviour of a function that has none.

The reasoning that survives is the ordering. Composition inside the window is the easy part -
`egui-wgpu` has `register_native_texture`, so a guest frame would be a full-screen image with
UI over it. Transport and cross-process synchronisation are real work. And a CPU copy is
7.91 MiB per 1080p frame, 498 MB/s at sixty - slow, but impossible to get subtly wrong, which
is the right first version.

**What this corrects is a habit, not a fact.** The description was assembled from what the
architecture implies rather than from what the code does, and it read as a status report.
Two of its load-bearing claims were about things that do not exist.


