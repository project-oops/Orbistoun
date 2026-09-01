# D295 - A stub policy that can write, so the loop stops needing a person to type Rust


**decided** · 2026-08-26 · because the last function was implemented by hand and should not have been

`sceKernelReserveVirtualRange` was measured entirely by the loop - slot, offset, the answer the
read is gated on, and that a mapped region behind it lets the guest through (D283, D284,
D291). Then a person wrote fifty lines of Rust that do exactly and only what the measurement
said. **That is the wrong division of labour**, and the project already says so in two places.

Principle 5: *"Rules and policy live in data, not code… the test: if answering 'what does this
function return?' requires a rebuild, it is in the wrong place."* And `orbistoun-thunk`'s own
statement of its limit, which the sweep was built around rather than removing: *"A stub policy
can change what a function **answers**. Nothing could change what a function **does** - and
both current walls turned out to be a side effect nobody performed."*

Both walls were a side effect nobody performed, and the answer was to build a sweep clever
enough to find that out. The other answer is to let a stub perform one.

**So the policy grows a second half.** Today an entry says what a function answers; now it may
also say what it *writes*:

```toml
[writes.sceKernelReserveVirtualRange]
slot = 0            # the argument holding the address to write through
region_bytes = 0x100000
```

Which is exactly the shape a sweep produces. `Finding::OutParameter { slot, offset, answer }`
becomes a policy entry with no judgement in between, so the loop can write one - and writing
data is a thing it may do, where writing code is not.

**The reservation happens in the service, not the thunk.** A thunk answering a call is on the
guest's own stack under principle 9's no-allocation rule; reserving address space there is the
wrong layer and the wrong moment. The service already builds the address space, so it reserves
the region up front and installs a *concrete base* per symbol index. The thunk then does one
store into `*args[slot]`, which is all a trampoline should ever do.

**`region_bytes` is honest about being assumed.** Nothing measured says how much space the
guest wants - the sweep sees where it faulted, not what it asked for (D291). A number in a
policy file is a value somebody can change without a rebuild and see what happens, which is
the whole reason policy is data. Deriving it from an argument would look more clever and
would bake an assumption into code instead of leaving it visible.

**What this does not become.** It is not a scripting language for stubs. One write, of one
base, into one argument - the shape the loop can measure and nothing wider. A policy that can
express arbitrary side effects is a program, and a program in a data file is the thing
principle 5 is trying to avoid, not achieve.

