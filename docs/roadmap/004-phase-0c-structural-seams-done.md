# Phase 0c - Structural seams *(DONE)*


Put the crate boundaries in place before there is code to restructure. Five pieces,
from D029-D035 and D048:

- **Split the graphics work.** `orbistoun-gpu` (translation, no `ash` dependency at
  all) and `orbistoun-gpu-vulkan` (the only crate that knows Vulkan exists). Makes
  leaking the host API into the translator impossible rather than merely discouraged.
- **`orbistoun-service`.** Extract the logic `orbistoun-cli` currently owns -
  `build_registry`, the survey flow assembled in `main.rs` - into the shared layer
  every shim calls. Gets more expensive with every command added, so do it now.
- **`orbistoun-proto`.** Shim-to-worker message types as serde data, defined
  separately from any transport.
- **`orbistoun-paths`.** Portable-first resolution per D038. Needed before the *first
  thing that writes*, and that is the GUI's settings at phase 2b - earlier than the
  traces at phase 4.
- **`orbistoun-overrides`.** Per-title settings per D048: one file per title, keyed by
  content hash, three layers merged per key. Structural for the same reason as the gpu
  split - the mechanism has to exist *before* the first title-specific need arises, or
  somebody hardcodes it and the pattern is set.

**Observable result:** `cargo tree` shows `orbistoun-gpu` with no path to `ash`; the
portable containment test passes; the CLI is a shim over the service rather than a
holder of logic; an override file changes behaviour with nothing title-specific
anywhere in the core.

