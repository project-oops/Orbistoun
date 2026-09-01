# D368 - Handoff field two is a pointer, and what the unknown fields hold is a setting


**decided** - 2026-08-29

D365 named field zero. This is the second field, and the knob that measured it.

### The measurement

Two runs of the same guest, differing only in what the fields nothing has established hold:

| fields | what happened |
|---|---|
| mapped markers | `__kernel_init` reads field 2, carries on, and passes `0x2001` to the resolver as a module handle |
| zero | `__kernel_init` reads field 2 and **faults dereferencing null**, at `image+0x44ef` |

So field 2 is **a pointer the runtime reads through**, not a scalar. Zero is not an acceptable
value for it, and the marker that stands in for it leaks into the handle the runtime then
passes back - `0x2001` is the marker's low half plus one, which is a number this emulator
manufactured rather than one the payload meant.

That second half is worth stating as a caution rather than a finding: **a marker that a guest
uses as data becomes a value the guest computes with**, and everything downstream of it is
about the marker rather than about the program. It names the field, which is what it is for,
and it makes the next few calls fiction.

### Why the fill is a setting rather than a constant

Markers name a field and zeroes let a guest check one, and neither is a claim about the
layout. Which one gets further depends on what the field is *for*, and that is exactly what is
not known - so it is `ORBISTOUN_HANDOFF_FIELDS`, defaulting to markers, recorded by
`orbistoun-env` so a verdict taken under either is never compared against one taken under the
other (principle 5, D181, D224).

Here markers get further and zero gets a cleaner fault. Both are worth having, and neither is
the answer: the answer is a value for field 2 that points at something real, and nothing has
established what that something is.

### What is now known about the structure

| field | what it is | how it is known |
|---|---|---|
| 0 | `sceKernelDlsym` | the guest called it with the string, out of its own `.rodata` (D365) |
| 2 | a pointer the runtime reads through | a null there faults; a mapped marker does not |

Every other field is unmeasured, and each is one run away from being described.

