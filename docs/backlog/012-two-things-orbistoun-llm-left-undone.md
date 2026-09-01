# Two things `orbistoun-llm` left undone


The crate exists (D212) and nothing calls it yet. Two gaps in it are worth recording
rather than rediscovering.

**Only NVIDIA accelerators report their memory.** `host.rs` shells out to `nvidia-smi`;
AMD and Intel report nothing, so those machines fall to the catalogue default and get a
smaller model than they could run. The better answer is already in this repository -
`orbistoun-gpu-vulkan` loads Vulkan at runtime and can enumerate device-local heaps on any
vendor - and was left out to avoid coupling an isolated crate to `ash` for one number. It
becomes nearly free the moment anything else wants a host description.

**`host.rs` should not live there.** It has no knowledge of models and no dependency on
the rest of the crate, precisely so it can be lifted whole. The reason to lift it is not
tidiness: D046 requires a run report to embed its own inputs so that a difference between
two runs is attributable to the change rather than to drift, and *the machine* is such an
input. Two runs compared across two machines are not comparable today and nothing says so.
Blocked on deciding where it goes - `orbistoun-core` is IO-free by contract and probing is
IO, so it is either a new crate low in the spine or a module of `orbistoun-report`.

