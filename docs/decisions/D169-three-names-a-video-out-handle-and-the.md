# D169 - Three names, a video-out handle, and the wall that has not moved


**decided** · 2026-08-21

Three items taken in order. Two landed, one turned out to be blocked for a reason worth
more than the work would have been (D168).

### Video-out handles are integers, not addresses

`sceVideoOutOpen` unimplemented answered `0x7FFF0001`, and the guest passed that straight
into `sceVideoOutRegisterBuffers2` as the port to register against - D125 with a display
attached.

Implemented as a small one-based index into a port table, and deliberately **not** an
address. Thread, lock and file handles are addresses because the guest dereferences them
(D151); this one is compared against zero and passed back, and nothing observed reads
through it. **The rule is "match what the guest does with it", not "always use an
address"** - which is worth stating, because three subsystems in a row wanting addresses
makes the fourth look like it should too.

One-based because callers test against zero, and a zero handle would read as "not open" to
code holding a perfectly good port.

`sceVideoOutRegisterBuffers` records how many buffers it was given and answers success. The
addresses are **not read** - there is no output surface, so a buffer registered here is one
nobody will present. Answering success rather than refusing is deliberate and reversible: a
guest that cannot register buffers stops setting up its display, while one that believes it
can proceeds to submit flips, which is where the GPU layer is eventually reached.

**Result:** the guest now gets past `RegisterBuffers2` and reaches
`sceVideoOutSetBufferAttribute2`, which it had never called before.

### Two more names, and why the obvious spellings missed

- `libSceSystemService::0xae2cfbe9f2389a7d` is `sceSystemServiceParamGetInt`.
- `libc::0xa75420e43cad1cdc` is **`snprintf_s`** - the seventy-six-call mystery.

The second is the interesting one. Its position was diagnostic on its own: consistently
between `sceKernelAllocateMainDirectMemory` and `sceKernelMapNamedDirectMemory`, taking a
stack address, which is the shape of something formatting the *name* the map call takes.
The hypothesis was right and every obvious spelling missed - `snprintf`, `sprintf`,
`vsnprintf` - because it is the C11 Annex K bounds-checked variant. **A correct hypothesis
about what a function does is not a hypothesis about how it is spelled.**

Declared rather than implemented, which makes it answer a count instead of the generic
error code a caller would otherwise use as a length.

### The wall has not moved, and that is now informative

`image+0x43c4`, `read of 0x0`, with `rbx` zero. It has survived: implementing the
filesystem, implementing `operator new`, blanket `default_return = "ok"`, real video-out
handles, and declaring `snprintf_s`.

Blanket success not moving it is the strongest signal - **it is not a stub return value**,
so no amount of tuning one will help. The leading hypothesis is now
`sceSystemServiceParamGetInt`: it takes an out-pointer that nothing writes, so the guest
reads whatever was on the stack there. That is a different failure from a bad return value
and would explain why every return-value experiment has been inert.

It needs a home - there is no system-service crate yet - which is a crate to add rather
than a concept to agree.

