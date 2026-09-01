# D173 - Call sites, which principle 9 asked for and nothing recorded


**decided** · 2026-08-21

Principle 9 states it plainly: *"which function" is the wrong question - "which call site" is
the right one. Every event carries a global monotonic sequence number and the guest return
address.* The sequence number was there. **The return address was not**, and had never
been.

It is free to capture. The trampoline already carries the stack pointer as the guest's
`call` left it, for the alignment measurement (D159) - and a `call` pushes the return
address and leaves `rsp` pointing at exactly it. The thunk reaches the trampoline by a
`jmp`, which pushes nothing, so that word is still on top when the recorder runs. One read,
no assembly change.

### Why it matters more than a count

A count says a title calls `memset` three hundred times. A call site says which three places
- and, decisively, **it is the same address space the fault's frame walk reports** (D172).
That is what lets a stack trace and a call trace be read against each other:

```text
frames:      image+0xdbfb -> image+0xde58 -> image+0xdc4f -> ... -> image+0x43c4
call sites:  sceKernelAllocateMainDirectMemory from image+0xdfc1
             snprintf_s                        from image+0xe021
             sceKernelMapNamedDirectMemory     from image+0xe047
```

The frames at `0xdb`-`0xde` and the call sites at `0xdf`-`0xe0` are the same neighbourhood:
**that cluster is the asset loader.** The chain then descends into an unrelated cluster and
faults there with no import call in between - so the fault is guest code working on data it
has just loaded, not a missing function.

None of that was visible from either trace alone.

