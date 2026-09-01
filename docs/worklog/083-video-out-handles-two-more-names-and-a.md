# Video-out handles, two more names, and a wall that will not move


Items two and three of three; item one was blocked by the harvest gap (D168).

**Video-out** (D169). `sceVideoOutOpen` was answering our error code and the guest was
passing it into `RegisterBuffers2` as the port. Implemented as a small one-based index -
and deliberately *not* an address, because the guest compares this one against zero and
passes it back rather than reading through it. Three subsystems in a row wanting
address-backed handles makes the fourth look like it should too; the rule is to match what
the guest actually does with the value.

The guest now gets past `RegisterBuffers2` to `sceVideoOutSetBufferAttribute2`, which it
had never reached.

**Two names.** `sceSystemServiceParamGetInt`, and - finally - the seventy-six-call
`libc::0xa75420e43cad1cdc` is `snprintf_s`. Its position had already given it away: always
between allocate and map, taking a stack address, formatting the name the map call takes.
The hypothesis was right and every obvious spelling missed, because it is the bounds-checked
C11 variant. Knowing what a function does is not knowing how it is spelled.

**The wall.** `image+0x43c4`, `read of 0x0`, `rbx` zero. It has now survived the filesystem,
`operator new`, blanket `default_return = "ok"`, real video-out handles and `snprintf_s`.
Blanket success leaving it untouched is the useful part: it is *not* a stub return value.

Leading hypothesis is `sceSystemServiceParamGetInt` - it takes an out-pointer nothing
writes, so the guest reads whatever the stack held. That is a different failure from a bad
return, which would explain why every return-value experiment has been inert. Needs a
system-service crate, which is a crate to add rather than a concept to agree.

