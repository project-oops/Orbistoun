# orbistoun-video

Video output: the guest's video-output library, the swapchain and the flip queue.

The guest opens an output, registers buffers, submits flips and waits on their completion. The
crate serves open, close, buffer registration and attributes, flip submission, flip events and
output configuration. Together with [orbistoun-gpu](../orbistoun-gpu/README.md) it produces a
title's visible output.

## Rule

**Flip completion always arrives.** A wrong flip-completion path is the classic cause of a
title that boots, renders one correct frame, and then appears to freeze, so completion is
settled before rendering. The flip queue is a counter that completes on submit - what a
headless emulator with no scanout can model honestly, and enough for a guest that polls flip
completion to proceed rather than hang.
