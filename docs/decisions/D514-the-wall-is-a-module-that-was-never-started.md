# D514 - The wall is a module that was loaded, placed, and never started

**measured** - 2026-09-03 (a deterministic run, the faulting instruction, and the export table)

PPSA02664 has walled at the same place for weeks: `read of 0x8` at the title's own
modules+`0x13dca44`, after 2077 import calls. D513 made the run reproducible, which is what
made it worth reading the fault properly rather than around it.

## What the instruction actually does

```text
4c 8b 7e 08    mov r15, [rsi+0x8]      <- rsi = 0x480001f0c330, r15 becomes 0
49 8b 4f 08    mov rcx, [r15+0x8]      <- FAULT: read of 0x8
80 79 19 00    cmp byte [rcx+0x19], 0
```

So it is not a null return being dereferenced. It is a **field at `0x480001f0c338` - inside
the title's own module data - that holds a pointer and holds zero**. Something that should
have written it never ran.

## What should have written it

The call stack is `0x4800013d5f00 <- 0x400000f23ac8 <- 0x400000f269c9`, and the two calls
immediately before are:

```text
sceKernelLoadStartModule(...) -> 0x40
sceKernelLoadStartModule(...) -> 0x41
```

`orbistoun-cli exports` puts the caller inside an export: `Il2CppUserAssemblies.prx` exports
an entry at `+0x13d5e90`, and the frame is `+0x13d5f00`, `0x70` into it. So that module is
mapped at `TITLE_MODULE_BASE`, its exported code is running, and the guest is executing it
having asked for it to be **started**.

Here is what `load_start_module` does for a `/app0` path:

```rust
let handle = NEXT_MODULE_HANDLE.fetch_add(1, Ordering::Relaxed);
write_int(args[5], 0);
return handle;
```

**A handle, and nothing else.** The `Load` half is done elsewhere and eagerly -
`place_title_modules` maps every module a title ships before the guest runs, which is why the
code is there to execute at all. The `Start` half has never existed.

## Which start, and the hypothesis that was wrong

The obvious candidate was `module_start`, the platform's named module entry point. Neither
module exports it:

```text
Il2CppUserAssemblies.prx   247 exports, no module_start (0xcf833c78728aa305), no module_stop
PS5Util.prx                  5 exports, neither
```

So that is dead. What both modules do have is the ordinary ELF mechanism:

```text
0x0c INIT        0x19 INIT_ARRAY        0x1b INIT_ARRAYSZ
```

and `orbistoun-elf` parses `init_array`, `init_arraysz`, `preinit_array` into `DynamicInfo` -
where **nothing outside the parser has ever read them**. A grep for `init_array` across every
crate but `orbistoun-elf` returns nothing.

That is the whole wall. The module's C++ static constructors have never run in any run of
this emulator, so a global one of them fills is zero, and the guest reads it three calls
later at a site with no visible connection to the load.

## Why it took this long to see, which is the part worth generalising

`sceKernelLoadStartModule` returned a plausible handle and said nothing. **A handle and a
silence are indistinguishable from a module that started** - the exact failure principle 3
forbids, in a function whose own name contains the half it does not implement.

So the gap is now reported, unconditionally, from the path every run ends on:

```text
orbistoun: 2 module(s) got a handle and were NOT started (nothing runs DT_INIT_ARRAY,
           so their constructors have not run): PS5Util.prx (handle 0x40),
           Il2CppUserAssemblies.prx (handle 0x41)
```

**This is a gap report, not a diagnostic.** Nothing switches it on and nothing intervenes; it
says what the run did not do. It is beside the intervention summaries in `persist` because
that is the one path every ending comes through (D513), not because it is one of them.

It also turned an inference into a measurement. The two paths were first identified by
arithmetic - the `snprintf` before each load returned `0x1f` and `0x2c`, and
`/app0/Media/Modules/PS5Util.prx` is 31 characters and
`/app0/Media/Modules/Il2CppUserAssemblies.prx` is 44. Right, and not evidence. The report
names them.

## What is deliberately not done here

**Running the initialisers.** It is the fix, it is a substantial one, and it needs two things
this change does not have: the placed-module table has to be reachable from the kernel shim,
which today only sees a path string; and calling `DT_INIT_ARRAY` means entering guest code
from a shim, which is a control-transfer this emulator does exactly once, at the entry point,
through `orbistoun-abi`.

Neither is a new concept and both are the obvious next piece of work. Splitting them from the
diagnosis keeps this decision about what was measured.

> **Done the same day, and both halves already existed - see D515.** Entering guest code from
> a shim is `thread::call_guest`, in this same crate, built for `call_once` initialisers.
> Reaching the module table has an exact precedent one screen away in `note_region` (D446).
> The estimate above was written from the shape of the problem rather than from the tree.
> Running the initialisers took the guest from 2,077 calls and a fault to 20,000,000 calls and
> its frame loop.

**And the test asserts the report, not the behaviour.** It cannot assert the constructors did
not run, because nothing in that crate loads a module. When something does run them, that
test should be *replaced* by one asserting they ran - not deleted quietly, which is how a
description of a gap outlives the gap (D510).
