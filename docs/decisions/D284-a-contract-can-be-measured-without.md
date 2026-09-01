# D284 - A contract can be measured without knowing what the function means


**decided** · 2026-08-26 · the `image+0xafc959` wall, opened without reading a line about it

The question this settles is whether the loop can go **detect -> understand -> satisfy**
without something in it recalling what a vendor function does. For one function it now has,
and every step is a command in this repository:

| step | how | external knowledge |
|---|---|---|
| the hash has a name | string harvested from a module, confirmed by hashing it | none - the hash is the oracle |
| `arg0` is a pointer, `arg1` is `0x100000` | `ORBISTOUN_DUMP` | none |
| the guest computes `*arg0 + arg1 - 0x20` | two `ORBISTOUN_WRITE` plants, addresses predicted first | none |
| success is required before it reads | `ORBISTOUN_RETURN` crossed with the plant (D283) | none |
| the base must be **mapped** | `ORBISTOUN_MAP` a region, plant its base | none |

The last row is the one that matters: with a real region behind the planted base the guest
**wrote successfully and carried on** - `FURTHER`, one new import, five more calls, and a new
fault of a different shape at a different address. The wall that stood through twenty-three
eliminations opened.

**Nothing in that chain needed to know what "reserve a virtual range" means.** The name is a
label on a hash. What the guest requires was read off its own behaviour: answer zero, write a
sixty-four-bit base into `*arg0`, and have at least `arg1` bytes behind it. An implementation
satisfying exactly that is not a guess about the platform - it is a transcription of a
measurement, and it is falsifiable by the same commands that produced it.

**What was *not* mechanical, stated plainly.** The hypothesis "the return may also have to be
right" was a person's idea, and it is the only thing here that did not come out of a tool.
It is also cheap to enumerate rather than think of: pointer-shaped arguments crossed with
plausible return values is a small grid, and D283 records that the current sweeps run one
axis at a time and therefore cannot see it. The step from here is a two-dimensional sweep,
not a cleverer guesser.

**The semantics stay `assumed` and that is not a weakness.** Whether this really reserves
address space, what `arg2` selects, whether `arg3 = 0x40000` is an alignment - none is
measured and none is needed. A contract that satisfies the guest is a testable claim; a
description of what the vendor intended is not, and only one of the two is admissible here.

