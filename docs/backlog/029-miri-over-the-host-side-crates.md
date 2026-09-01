# Miri over the host-side crates

`orbistoun-mem`'s bookkeeping and the trace sink's atomics are worth checking under
Miri. Guest execution never will be - it runs native code Miri cannot interpret - so
this is scoped to the pure layers. `rust-src` is already pinned for it.

## Ergonomics

