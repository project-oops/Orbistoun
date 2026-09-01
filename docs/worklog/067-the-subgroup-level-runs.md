# The subgroup level runs


The third of three. 601 workspace tests green, no lints in the shader-side crates.

`Fidelity::Subgroup` is the per-lane model with a ballot bolted on (D146). Two device tests:
the module loads at all - it declares two capabilities, an input built-in and a group
operation, any of which a driver rejects outright if malformed - and a cleared execution
mask suppresses the write after it, which is what the level exists for.

### Surprises

- **The emitter's own checker caught the bug before the driver did.** `OpTypeVector`'s shape
  entry listed the result id as one of its own *uses*, so the checker reported it used
  before definition. That is exactly the class of fault `Builder::check` was built for after
  a bad module crashed a driver, and this is the first time it has paid out on a new opcode.
- **Every fidelity level is built now**, which broke the test asserting one was not. It was
  the only thing asserting the subgroup level was unimplemented, so leaving it would have
  quietly required it to stay that way. Replaced with the property that still holds: `Auto`
  never escapes as itself.
- **A doc block landed between a struct's doc comment and its derive**, silently moving
  `#[derive(Debug)]` onto the wrong type. It compiled until the other type gained its own.

### Outstanding

Several guest lanes per invocation - what a 64-lane shader needs on this 32-wide device -
is not built. The module reports the width it needs and the caller declines, so nothing runs
wrong in the meantime. The local data share and the lane-index count are still refused at
this level.

