# 2026-09-03 - (/loop) The wall named: a module that was loaded, placed, and never started

```
suites 123   tests 2003   clippy/fmt/identity clean
```

Arrived by hand again - the twentieth wakeup that did not fire. First tick of a 20-minute
fixed-interval loop.

## The wall, read properly for the first time

D513 made the run reproducible, which is what made it worth reading the fault rather than
around it. The faulting instruction:

```text
4c 8b 7e 08    mov r15, [rsi+0x8]      <- rsi = 0x480001f0c330, r15 becomes 0
49 8b 4f 08    mov rcx, [r15+0x8]      <- FAULT: read of 0x8
```

Not a null *return* being dereferenced - a **field inside the title's own module data that
holds a pointer and holds zero**.

`orbistoun-cli exports` puts the caller inside an export: `Il2CppUserAssemblies.prx` exports
an entry at `+0x13d5e90` and the frame is `+0x13d5f00`. So the module is mapped, its code is
running, and the two calls before the fault are both `sceKernelLoadStartModule`.

## What that function does

```rust
let handle = NEXT_MODULE_HANDLE.fetch_add(1, Ordering::Relaxed);
write_int(args[5], 0);
return handle;
```

**A handle, and nothing else.** `place_title_modules` does the `Load` half eagerly before the
guest runs - which is why the code is there to execute. The `Start` half has never existed.

## `module_start` was the wrong guess; `DT_INIT_ARRAY` is the answer

Neither module exports it:

```text
Il2CppUserAssemblies.prx   247 exports, no module_start (0xcf833c78728aa305)
PS5Util.prx                  5 exports, neither
```

Both have `INIT`, `INIT_ARRAY` and `INIT_ARRAYSZ`. `orbistoun-elf` parses all three into
`DynamicInfo`, and **a grep for `init_array` across every crate but the parser returns
nothing**. The modules' C++ static constructors have never run in any run of this emulator.

## The gap is now reported instead of silent

`sceKernelLoadStartModule` returned a plausible handle and said nothing - a handle and a
silence are indistinguishable from a module that started, in a function whose own name
contains the half it does not implement. Unconditional now, from `persist`:

```text
orbistoun: 2 module(s) got a handle and were NOT started (nothing runs DT_INIT_ARRAY,
           so their constructors have not run): PS5Util.prx (handle 0x40),
           Il2CppUserAssemblies.prx (handle 0x41)
```

A gap report, not a diagnostic: nothing switches it on and nothing intervenes.

**It also turned an inference into a measurement.** The two paths were first identified by
arithmetic - the `snprintf` before each load returned `0x1f` and `0x2c`, and the two paths are
31 and 44 characters. Right, and not evidence.

## Broken and watched to fail

```text
the load is not recorded -> "a module was asked for, so the run is no longer quiet"  FAILED
```

## Not done, deliberately

**Running the initialisers.** It needs the placed-module table reachable from the kernel shim,
which today sees only a path string, and it means entering guest code from a shim - a control
transfer this emulator does exactly once, at the entry point, through `orbistoun-abi`. Neither
is a new concept; both are the next piece of work.

The test asserts the *report*, not the behaviour, and says so. When something runs the
initialisers it should be **replaced** by one asserting they ran, not deleted quietly (D510).

Decision: [D514](../decisions/D514-the-wall-is-a-module-that-was-never-started.md).
