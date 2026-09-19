# 683. Eight doc claims corrected against the code they describe

**2026-09-17** — inbox `-6323`: eight places where a document claimed *less* than the code does — a
count, an emptiness or a refusal written beside a list that later grew. A doc understating the code
misleads in the same direction as one overstating it: by being believed. Five are the one failure
repeated; two (items 1 and 2) are a chain where the correction reached neither end.

## The rendering backend is not a stub (items 1–4)

The Vulkan backend's `execute` has **seven** handled arms — `SetRenderTargets`, `SetViewport`,
`BindShader`, `BindBuffer`, `Dispatch`, `Draw`, `DrawIndexed` — and only `ClearColour` and `Fence`
fall through; `present` is refused separately. The module doc (`orbistoun-gpu-vulkan/src/lib.rs`)
already said so; four things around it did not:

1. **`crates/orbistoun-gpu-vulkan/README.md`** — *"Models: nothing yet. Every command is refused"*
   and *"the rendering backend is still a stub - every draw and present is refused"*. Restated to
   list what executes and name the three still refused (`ClearColour`/`Fence`/`present`).
2. **`docs/features/graphics.md`** — the same claim, and it cited it *to that README*. Repointed at
   the module doc per acceptance, and the "every draw and present is refused" line replaced with the
   draws-and-dispatch-execute reality.
3. **`orbistoun-gpu-vulkan/src/lib.rs:648`** — the `execute` fallthrough comment listed *"indexed
   draws, render targets, viewport, clears"*; three of those four are handled arms above it. Now
   names only `ClearColour` and `Fence`.
4. **`orbistoun-gpu/src/lib.rs`** — *"Nothing is translated to Vulkan yet."* Now says the command
   stream and shaders translate to backend-neutral commands here and the sibling crate executes them.

## Counts and emptinesses that a list outgrew (items 5–8)

5. **`orbistoun-gpu/src/agc.rs`** — *"Forty-four of the fifty-seven … are implemented."*
   `implementations()` returns 46. Per acceptance the prose now carries **no count** and points at
   `tests/dcb_wiring.rs`, which asserts it — one place a comment cannot drift from.
6. **`orbistoun-gpu/tests/vocabulary.rs`** — *"There are no captures yet."* Three sit in
   `tests/captures/` and the suite reads them; reworded to describe the small corpus it checks. (The
   dated worklog 046 that also says it is left as the historical record it is.)
7. **`docs/PROJECT_STATUS.md`** — *"`sceKernelDlsym` … which nothing implements."* It is implemented
   (`orbistoun-kernel/src/lib.rs:5572`). The observation survives — dlsym-resolved names land on
   stubs while the same names resolve through the import table — but its stated cause was false; now
   it reads as the two resolution paths disagreeing, a path never *reconciled* (not never exercised).
8. **`docs/roadmap/015-…`** (G15 row and the paragraph) — *"the backend queries four device features
   today and not that one"* of `textureCompressionBC`. `compute.rs:557` requests it and `:621`
   reports it; both now say the backend queries it and the report reflects it — the capture-free
   first step is done.

## Gate state

`orbistoun-gpu` and `orbistoun-gpu-vulkan` build and test green (comment-only changes);
`./bin/orbistoun prose` exit 0; `status --check` exit 0 (the PROJECT_STATUS edit is prose, outside
the generated blocks); `cargo fmt --check` clean; identity scan clean. No commit.
