# D495 - The game asks for its modules by name, and `LoadStartModule` starts nothing

**measured** - 2026-09-03 (the guest's own arguments, and 318,490 untouched words)

Four ticks have gone into what fills a `.bss` global in `Il2CppUserAssemblies`. Three
mechanisms died - constructors (D491), `module_start` (D492), a failed `.data` copy (D493).
This is the fourth, and the guest names it out loud.

## What the guest asks for

```text
sceKernelLoadStartModule("/app0/Media/Modules/PS5Util.prx", 0, 0, 0, 0, 0)  -> 0x40
sceKernelLoadStartModule("/app0/Media/Modules/Il2CppUserAs…", 0x34, <pointer>, 0, …)
```

Twice, by full path, for the two modules D482 goes looking for. The second carries a value and
a pointer in the second and third arguments where the first passed zeros - **not interpreted
here**, because the parameter names are not established and a guess would be an invented arity.

## What orbistoun does about it

```rust
if path.starts_with("/app0/") {
    let handle = NEXT_MODULE_HANDLE.fetch_add(1, Ordering::Relaxed);
    write_int(args[5], 0);
    return handle;
}
```

A handle, a success code, and **nothing else**. No load, no start. That is honest as far as it
goes - it was written when nothing could load a module at all, and answering a handle for a
path that exists beats refusing one that does.

It is no longer as far as it goes. D482 places these modules, D489 relocates and binds them, and
the guest runs their code - so by the time this call arrives the module is *loaded* and has
never been *started*.

## Which the memory confirms, quantitatively

A snapshot of the module's entire `.bss` - segment 4's zeroed run, `0x26e0dc` bytes - across a
whole run:

```text
0x480001f0d2ac  0x0 -> 0x7070612f00000000
0x480001f0d2b4  0x0 -> 0x00616964654d2f30
(318490 word(s) unchanged)
```

Two words out of **318,492**, and decoded they are the string `/app0/Media`. Nothing else in
two and a half megabytes of the module's own state is ever written.

That is not a module whose initialisation went wrong. It is a module that was never initialised.

## And nothing is refusing it

The obvious alternative - that initialisation starts, hits something unimplemented, and bails -
was tested. The run has exactly two unimplemented calls, `scePthreadSetprio` and
`scePthreadSetaffinity`. Forced to answer success, three interleaved samples:

```text
44/46 distinct, 2077/2080 calls, fault at +0x13dca44   - identical, within the known ±3
```

**No movement.** Whatever is missing is not a refusal this run makes.

## What this says about D482

D482 decided a title's module is *found by searching the title's tree for the name that imports
it*, because the vendor tables carry bare names and no paths. That is still true of the import
tables, and the search is still what allows binding before the guest runs.

But the paths are not unknown. **The guest supplies them, in full, at runtime** - and the
search reconstructs by convention what this call states outright. Where the two disagree, this
call is the authority: it is the platform's own interface and the game's own words.

## What is still not established

What "start" *does*. `DT_INIT_ARRAY` is empty (D491) and no `module_start` is exported (D492),
so whatever the platform runs on a module is neither of the two obvious answers. That question
is now well-posed for the first time - it is about one call with a known name and known
arguments, rather than about a global that happens to be zero.
