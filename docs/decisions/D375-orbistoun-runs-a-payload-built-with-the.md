# D375 - orbistoun runs a payload built with the real toolchain


**decided** - 2026-08-29

Two routes were open on the handoff structure. The first was cheap and answered a question by
saying no; the second answered a much bigger one.

### Route one: stubs behind a field, which did not fire

A marker behind a field says the guest *read* that member and cannot say what it then did
with it - using it as a function pointer ends the run on an unmapped address with the
arguments already gone. So the page behind each field was filled with stubs instead: one per
member, each naming its field and its offset, answering zero and printing how it was called.

**Nothing called one.** `__kernel_init` reads what field two points at and does not call
through it. That is a real answer - field two is not a table of functions - and it cost one
variant on an enum.

### Route two: build a payload with the SDK and watch it

The open toolchain builds, on this machine, in the WSL2 Ubuntu that already builds the
conformance probe. A payload whose `main` was written **here** and linked with the real SDK
fails **identically** to `klogsrv`: the same two resolutions, `sceKernelDlsym` then `getpid`,
then the same wild jump.

So the wall is not in any payload. It is in the runtime every payload links.

### And then the result that matters

The same source, built with the same SDK but entered our own way - a `_start` written here
that calls `main` directly - **runs**:

```text
orbistoun probe: reached main without the runtime
orbistoun probe: main ran
```

Imports resolved, `puts` served, two calls, both on a conforming stack. It ended on the time
limit because the `_start` written here ends in a loop, which is the payload doing what it was
told.

**orbistoun executes payload-SDK binaries end to end.** The entire remaining gap between it
and `klogsrv` is one function: the runtime's own initialisation.

### The licence line, and where it is

The SDK is GPL-3.0 and this project is MIT/Apache-2.0, so it is used as a **build tool** and
never as a source: a payload is compiled with it and observed, exactly as a commercial title
is loaded and observed. Nothing was read out of its `crt` or its headers, and nothing here was
written while reading either. The `main` in the probe is this repository's own.

### A sweep must not be a rebuild

What is left is a sweep: try a value in a field, run, see whether the runtime gets further.
That is one question per run, and one question per run must not be a rebuild per run
(principle 5).

So `[entry] handoff-fields = [[2, 0]]` puts a literal in a named field, applied over whatever
the argument block produced. Naming field zero replaces the resolver, which is a thing
somebody may want to try and should not have to edit code to try. The first use of it already
paid: a null in field two faults at `__kernel_init+0x15`, which is a tighter address than any
run before it.

