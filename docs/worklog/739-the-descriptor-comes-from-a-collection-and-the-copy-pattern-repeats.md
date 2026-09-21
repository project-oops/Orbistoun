# 739. The descriptor comes from a collection, and the copy pattern repeats

**2026-09-20** — continuing worklog 738's descriptor trace one level up. The register-group descriptor is
not a lone object built inline; it is an element of a **collection**, retrieved by an accessor, and the
copy pattern that faults recurs at several call sites. This narrows what is wrong but also shows the wall
is deep guest structure, not a single missing call.

## The pattern repeats, and the descriptor is fetched from a container

The function above the faulting one (`0x38xxx`) does the same thing the faulting site does:

```
0x38310  lea   r15, [r13+0x38]      ; a container inside the shared object r13
0x38314  mov   esi, 2
0x3831c  call  0x3fdd0              ; accessor(container, 2) -> rax
0x38321  mov   r12, [rax]           ; r12 = the descriptor
0x38332  movzx edx, byte [r12+0x5b] ; present flag for group 0
0x38341  jz    0x38350              ; absent -> skip
0x38343  mov   rsi, [r12+0x18]      ; data pointer for group 0
0x3834b  call  0x42d90              ; copy it
0x38350  movzx edx, byte [r12+0x5c] ; group 1 flag, [r12+0x20] data, ...
```

So the descriptor (`r12`/`r14` depending on the frame) is **element N of a container** at `[r13+0x38]`,
fetched through `0x3fdd0` - a `std::vector`/map-style accessor. Its shape is two parallel arrays: present
flags at `+0x5b, +0x5c, …` and data pointers at `+0x18, +0x20, …`. For each group whose flag is set, the
guest hands its data pointer to `0x42d90` to copy. Group 0's flag is set and its pointer is null - the one
inconsistency, now located inside a collection element rather than a standalone object.

## What this says about the shape of the wall

The workload build walks a container of these descriptors, and copies each present group's data. The
producer that fills a descriptor is almost certainly inlined (worklog 723's "six non-exports": the title
runs its own copy, so orbistoun's `sceAgcDcbSetCxRegistersIndirect` is not even called on this path).
That means the divergence is not "orbistoun answered a call wrong" in the obvious sense - it is that the
inlined producer, running the title's own code, read some orbistoun-provided value and wrote a present
flag without a data pointer. Finding that value is finding which single input to the inlined builder
orbistoun gets wrong, which is a needle in the workload-construction haystack.

## Honest state and the next handle

This is worklog eleven on one wall (719 onward), and each tick narrows it by a level: null container ->
descriptor data pointer -> collection element with flag set and pointer null. That is real narrowing, but
the remaining question - which orbistoun input the inlined producer misreads - is not reachable by
walking the call chain up indefinitely. The productive next handle is to **get the descriptor's address**
and watch or dump it: with the collection element's address, `[desc+0x18]` and `[desc+0x5b]` can be read
directly, and a watchpoint on `[desc+0x18]` (a heap address, unlike the stack watchpoints D522 warns
churn) would catch the write - or the absence of one. Recovering that address from the saved registers on
the fault stack is the next concrete step; the descriptor is reachable through the container at
`[r13+0x38]` if `r13` (the shared graphics object) can be pinned.

## Gate state

No code changed - `ORBISTOUN_PEEK` disassembly only. `./bin/orbistoun check` was green as of worklog 736;
identity scan clean. No commit.
