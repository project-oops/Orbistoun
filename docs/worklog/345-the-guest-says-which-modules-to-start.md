# 2026-09-03 - (/loop) The guest says which modules to start, and we do not start them

```
tests   1991  ->  1991   (four experiments, no code)
```

Four ticks on one null global. Three mechanisms dead: constructors (D491), `module_start`
(D492), a failed `.data` copy (D493). This tick found the fourth by asking cheaper questions
first.

## The cheap discriminator, which was decisive

Rather than dump registers at the two module calls, snapshot the module's whole `.bss` -
`0x26e0dc` bytes - and ask what the guest changes in it across a run:

```text
0x480001f0d2ac  0x0 -> 0x7070612f00000000
0x480001f0d2b4  0x0 -> 0x00616964654d2f30
(318490 word(s) unchanged)
```

**Two words out of 318,492**, and decoded they spell `/app0/Media`. Two and a half megabytes of
the module's own state, untouched.

That reframes everything: not an initialisation that went wrong, a module that was never
initialised. And it cost one environment variable, because the cheap half of the pair already
existed (D223) - the same lesson as D494, one tick later.

## The obvious alternative, killed with three samples

Perhaps initialisation starts and bails on something unimplemented. The run has exactly two:
`scePthreadSetprio` and `scePthreadSetaffinity`. Forced to answer success:

```text
44/46 distinct, 2077/2080 calls, fault at +0x13dca44
```

Identical, inside the known ±3. **Nothing in this run is refusing the module.**

## Then the guest said it plainly

```text
sceKernelLoadStartModule("/app0/Media/Modules/PS5Util.prx", 0, 0, 0, 0, 0)  -> 0x40
sceKernelLoadStartModule("/app0/Media/Modules/Il2CppUserAs…", 0x34, <ptr>, 0, …)
```

Twice, by full path, for the two modules D482 goes hunting for. And orbistoun's answer:

```rust
if path.starts_with("/app0/") {
    let handle = NEXT_MODULE_HANDLE.fetch_add(1, Ordering::Relaxed);
    write_int(args[5], 0);
    return handle;
}
```

A handle, a success code, nothing else. Written when nothing could load a module at all, where
answering a handle beat refusing a path that exists. Since D482 and D489 the module is *loaded*
by the time this call arrives - and has still never been *started* (D495).

## Which corrects how I have been thinking about D482

D482 searches the title's tree because the vendor import tables carry bare names and no paths.
That is true and the search is still what lets binding happen before the guest runs.

But **the paths were never unknown**. The guest hands them over in full at runtime, and the
search reconstructs by convention what this call states outright. Four ticks were spent asking
what starts a module while the guest was calling a function with *Start* in its name, twice, in
every run.

## What is still open

What "start" actually does. `DT_INIT_ARRAY` is empty and no `module_start` is exported, so it is
neither obvious answer. But the question is now about **one call with a known name and known
arguments** rather than about a global that happens to be zero, which is the first time it has
been well-posed.

The second call's `0x34` and pointer are recorded and deliberately not interpreted - the
parameter names are not established here, and naming them would be an invented arity.

## State

`cargo test --workspace` green - **117 suites, 1991 tests**, 0 failures. fmt clean, identity
scan clean. No code changed this tick.

Nothing committed. The day holds worklogs 292-345 and D466-D495.

**Next**: what the platform does on `LoadStartModule` for a module the loader has already
placed. The sibling repos are worth grepping first - obSCEne builds `sce_module` stubs and
prosperous invokes `elfldr`'s load/launch, so one of them may already know.
