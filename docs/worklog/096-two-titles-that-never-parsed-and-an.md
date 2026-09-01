# Two titles that never parsed, and an abort reported as an illegal instruction


**The previous generation parses** (D176). Two of six titles were refused with a named
error - and nobody had ever checked whether the refusal was necessary. The headers are the
same shape: same version, mode, endianness, attributes, key type, plausible segment count,
descriptors that parse. They differ in four magic bytes.

Four lines. Both titles now load completely and execute guest code, 53 calls and 131. The
corpus went from four runnable to six. The generation is reported rather than flattened -
they parse the same, but a previous-console title is a different emulation problem.

A refusal written from a reasonable assumption, documented honestly, never measured, and it
cost a third of the corpus for as long as it stood.

**Then both new titles failed identically** - same fault address, same 53 calls, same four
frames. Identical across unrelated titles is the D152 signature: a shared path, not title
code. The call tail with call sites named it in one line:

```text
52  libc::abort   arg0=0xc10   from 0x400001595bc9
```

`abort` called *from the exact address that then trapped*. It is `noreturn`, so the
compiler emits an unreachable trap after the call; `abort` was undeclared, fell to the
default stub, returned, and execution walked into that trap. The emulator was reporting
`illegal instruction` at a meaningless address while the guest was giving up on purpose
(D177).

Fixed with a handler in `orbistoun-core` that the worker installs and the subsystems call -
the subsystem does not know whether a trace is being written, and the worker sits above it
and cannot be called downwards.

And a third outcome that was being reported as the second: a run ends by faulting, by the
time limit, or by the guest stopping itself. With no field for the third, `abort` was
described as *"ran to the time limit"*. It now reads `the guest called abort`.

