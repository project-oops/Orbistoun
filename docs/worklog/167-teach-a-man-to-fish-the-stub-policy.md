# Teach a man to fish: the stub policy learns to write


`sceKernelReserveVirtualRange` was measured end to end by the loop - slot, offset, the answer
the read is gated on, and that a mapped region behind it lets the guest through. Then **a
person wrote fifty lines of Rust doing exactly and only what the measurement said**, which was
the wrong division of labour and got called out as such.

The project had already written down why. Principle 5: *"if answering 'what does this function
return?' requires a rebuild, it is in the wrong place."* And `orbistoun-thunk`'s statement of
its own limit, which the entire two-dimensional sweep was built to work *around* rather than
remove: *"A stub policy can change what a function **answers**. Nothing could change what a
function **does** - and both current walls turned out to be a side effect nobody performed."*

Both walls were a side effect nobody performed. So a stub can perform one now (D295).

```toml
[policy.overrides]
sceKernelReserveVirtualRange = "ok"

[policy.writes.sceKernelReserveVirtualRange]
slot = 0
region_bytes = 0x200000
```

The hand-written implementation was **deleted**, and that policy alone produces the identical
result:

```
imports  25 distinct (+2), 232 calls (+10)
fault    image+0xafcc08   (was image+0xafc959)
verdict  FURTHER  executed code it could not reach before
```

Every value in those fourteen lines came out of the loop's own measurement, and the shape is
`Finding::OutParameter { slot, offset, answer }` with no judgement in between - so the loop can
write one itself. Writing data is a thing it may do; writing code is not.

### How little new code it needed

Almost none, because the write already existed as a **diagnostic**. `install_forced_writes`
plants a value through an argument slot and its doc says why it exists - to answer "is arg0 an
out-parameter the guest expects filled?" - and calls itself *"a diagnostic, not a feature"*.
Promoting that from a question asked once to an answer that persists is a second table and a
shared `apply_writes`, not a second copy of the store, the bounds check and the refusal
counting.

The reservation happens in the **service**, not the thunk: a trampoline runs on the guest's
stack under principle 9's no-allocation rule, and the layer that builds the address space is
the one that should take a region from it.

### Three things caught before they shipped

- **`Reservation` releases its range on drop.** Taking one and testing `.is_ok()` unmaps it
  before the guest ever sees the base, and the guest then faults at exactly the address the
  placeholder did - which reads as "the policy did nothing" rather than as a bug. `forget` is
  load-bearing and says so.
- **The config section is `[policy]`, not `[stub_policy]`.** The first attempt used the field
  name from `FileConfig`'s Rust declaration, serde ignored the unknown section in silence, and
  the run came back `BACK`. Diagnosed by reading the struct rather than by guessing.
- **Inserting `POLICY_WRITES` above `install_forced_writes` stole its doc comment**, leaving a
  public function undocumented and the diagnostic's reasoning attached to the wrong thing.
  Caught by `-W missing-docs`.

