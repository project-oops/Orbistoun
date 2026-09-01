# D194 - The run dumps what the guest was pointing at


**decided** · 2026-08-22

A trace says an unimplemented function was called and with what first argument. It does not
say what that argument *was* - and for an out-parameter or a descriptor struct, that is the
whole of the information. `sceKernelDirectMemoryQuery` was understood because the guest
passed a structure size and somebody read it by hand (D083). This is that, automatically.

### Bounded on every axis, because the alternative changes the program

Recording must not alter what it observes (principle 9), so: only imports nothing
implements, only their first two calls, only arguments pointing into memory this process
mapped, thirty-two bytes each, a fixed ceiling on total dumps, and no allocation anywhere on
the path.

The mapped-range check does double duty. It is the **safety** precondition - an argument
that is not a pointer is usually a length, and dereferencing it would fault inside the
emulator, turning a diagnostic into a crash unrelated to the guest - and it is the
**filter** that stops a count being mistaken for an address.

Captured at call time, not at collection time. The contents do not survive: a guest passes a
stack address, the call returns, and the frame is reused within microseconds. Reading later
would produce a confident, precisely wrong answer.

### It paid immediately

`sceAgcCreateShader`, the top finding on the furthest title, in one run:

```
arg0 at image+0x4f6c68 = 00 00 00 00 ...                    <- the out-parameter
arg1 at image+0x1b9420 = 31 32 33 34 18 00 00 00 d8 00 ...  <- magic "1234", 0x18, a size
arg2 at image+0x1c2a00 = 02 00 a0 bf 09 0c 04 7e ...        <- shader bytecode
```

A magic, a header size, and a payload length that differs per call. That is the calling
convention of the shader submission entry point, derived without reading a disassembly and
without anybody deciding what to look at.

An `libSceAgcDriver` import took a pointer to `"Color %d"` - it carries a debug name, which
says a great deal about what kind of call it is.

### What it cost, and what could not be measured

The first version asked `is_implemented` separately, which added a table lookup to **every**
call including the implemented ones - and the busiest title makes tens of millions of those.
The handler is now looked up once and the dump decided from the result, so the implemented
path is exactly as it was.

Whether any cost remains is **unmeasurable**. Three identical runs of that title returned
77.5M, 75.8M and 87.6M calls - a fifteen per cent spread from machine load alone, because
the limit is wall-clock. D181 recorded that as a determinism gap and deferred the fix; it is
now preventing a straightforward question about a change from being answered at all.

A call budget instead of a time limit would make the count deterministic and the *duration*
the varying quantity, which is the right way round for measuring throughput. Recorded here
rather than done, because it is a change to how every run is bounded.

