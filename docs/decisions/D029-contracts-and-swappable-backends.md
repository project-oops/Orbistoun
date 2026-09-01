# D029 - Contracts and swappable backends, abstracted at guest semantics

**decided** · 2026-08-19

Nothing is hardcoded where a contract will do. One graphics backend exists today;
adding another must never require surgery on the translator.

**The refinement that makes this work:** abstract at the level of *what the guest
asks for*, not what the host API provides. A `RenderBackend` trait designed by
looking at Vulkan ends up carrying descriptor sets, render passes, and explicit
barriers - and then a second backend fits badly anyway, because one API's model got
baked into the contract. Each backend maps guest semantics onto its own primitives.
Same rule for audio (submit samples at a rate, not WASAPI concepts), input (pad
state, not XInput), and filesystem (guest path semantics, not Win32).

**The test for whether a seam is premature:** if it only pays off hypothetically, it
is speculation. If it buys testability or swappability *now*, it is structural. The
render backend passes - a `RecordingBackend` lets command-stream translation be unit
tested with no GPU, no window, and no driver, on CI and in the Linux VM.

Any string that crosses a boundary is a named constant declared once - env var
names, path components, config keys, protocol message names. The
`pub const ENV_PORTABLE` pattern, carried from another project of mine.

