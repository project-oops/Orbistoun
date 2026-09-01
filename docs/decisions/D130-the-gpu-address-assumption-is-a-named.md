# D130 - The GPU-address assumption is a named function, not a silence


**Status:** assumed

`pipeline::guest_address_of` converts a GPU virtual address to a guest one. It is the
identity function.

Everything in that module reads a shader at an address a hardware register named, through
a trait that reads the *guest's* address space. Those are two address spaces and the code
treats them as one. The loader side has established that a guest virtual address is the
host address - identity, and load-bearing - but whether a GPU virtual address is also
that number is not established. The console shares one coherent memory pool between both
processors, which is the reason to expect it. Expecting is not knowing.

A function rather than nothing, so checking it later is one edit rather than a search,
and so a reader of a failed shader read knows which assumption to suspect first - the
failure message says so directly.

**Not a trait.** There is no second implementation and no evidence there will be one;
inventing a seam for a hypothetical is speculation (principle 12). This is a
known-uncertain constant with a single call site, and that is what it is modelled as.

If the assumption is wrong the failure is loud: the read finds nothing mapped, or finds
bytes that do not decode. That is the right shape for a guess to fail in, and is why this
is a named assumption rather than a blocker.

