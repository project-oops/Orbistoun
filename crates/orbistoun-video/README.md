# orbistoun-video

Video output - the guest's video-output library. Swapchain and flip queue.

**Models:** declarations for open, close, buffer registration, flip submission, and
flip status.

**Deliberately fakes:** the flip path. Opening an output, closing it, and registering
buffers are implemented; submitting a flip and reporting its completion are not.

**Design note.** The guest registers buffers, submits flips, and waits on their
completion. Getting flip completion wrong is the classic cause of a title that
boots, renders one correct frame, and then appears to freeze - so the completion
path deserves attention before the rendering path looks right.

Together with [orbistoun-gpu](../orbistoun-gpu/README.md) this is the pair that
produces the first visible output, which makes it the natural first milestone after
the loader.

**Status:** four functions implemented - open, close, and both buffer-registration
entry points. Remaining arities provisional. `docs/ROADMAP.md` phase 6.
