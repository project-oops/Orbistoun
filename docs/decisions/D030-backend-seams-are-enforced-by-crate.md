# D030 - Backend seams are enforced by crate boundaries, not discipline

**decided** · 2026-08-19

`orbistoun-gpu` holds command-stream and shader translation and **does not depend on
`ash` at all**. `orbistoun-gpu-vulkan` is the only crate that knows Vulkan exists.

Leaking the host API into the translator is then not discouraged but *impossible* -
the dependency is not there to leak, and `cargo` enforces it rather than code review.
A future second backend is a new crate rather than surgery. Audio gets the same
treatment when it lands.

Done before the code exists, because restructuring costs nothing now and everything
later.

