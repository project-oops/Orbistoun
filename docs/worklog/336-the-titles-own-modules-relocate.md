# 2026-09-03 - (/loop) The title's own modules relocate

```
tests   1980  ->  1984
```

D482 placed them, D484 gave them a shared stub table, and this links them. All three corpus
titles relocate every module they ship, completely:

```text
PPSA02664   Il2CppUserAssemblies   346028 applied, 0 unresolved, 0 tls-deferred, 0 unsupported
            PS5Util                    11 applied, 0 unresolved
            libc                     1896 applied, 0 unresolved
PPSA03416   Il2CppUserAssemblies   381319 applied, 0 unresolved
            libSceAmpr                247 applied, 0 unresolved
PPSA25872   Il2cppUserAssemblies    96419 applied, 0 unresolved
            libc                     3028 applied, 0 unresolved
```

**A placed module was bytes; a relocated one is code.** Every internal pointer in
`Il2CppUserAssemblies` now reads as an address rather than as a link-time offset, which is what
had to be true before an address inside it was worth handing to anybody.

## What "0 unresolved" does and does not say

It says every relocation found a target. It does **not** say every import is implemented -
these modules import from `libc` and `libkernel`, and those resolve to *stubs* in the shared
table, which is a target like any other. A stub that reports being called is the right answer
there and the count would look identical if all of it were implemented, so the number is worth
having and worth not over-reading.

## The three pieces, each with a caller before it was believed

**`slot_ranges` gained `OffsetResolver`.** A relocation inside module *M* names *M*'s own symbol
index, so something has to add `offset(M)` before the shared table is consulted. It **refuses
rather than wraps** on overflow: a wrapped index answers a different module's slot, binding the
relocation to an implementation written for another symbol - silently, for the whole run. An
unresolved relocation is counted and reported; a wrong one is not. Watched failing by swapping
`checked_add` for `wrapping_add`.

**`build_data_blocks_for`** builds one block across every module with shifted indices, so the
named map merges as a consequence - which is what `install_data_symbols` needs, being the last
`OnceLock` on this path. It also **reports a name two modules both import as data**: they get
separate storage, as they must, but a lookup by name can only answer one of them and which is
not something that layer can decide.

**`link_title_modules`** ties them together in D482's order - place all, collect all exports,
relocate all. None of the three was written without the next one calling it; an uncalled
abstraction is the thing principle 12 warns about, and two of these would have been exactly
that if the slice had stopped a step earlier.

## What is still not true

**Nothing here is wired into `run`.** The worker still takes the single-module path, so a guest
does not yet get these modules - `orbistoun-cli imports <eboot> --own --linked` is the only
caller. That is the next unit and it is a small one now: the worker's sequence has to place and
link the title's modules before it relocates the executable, and pass the shared table on.

**The executable is not relocated here**, deliberately. It is module 0 and takes slot zero, but
relocating it is part of entering it, which is the worker's sequence rather than this one's.

**`install_data_symbols` is called by a reporting command**, which is harmless in a one-shot CLI
and would not be in a process that later ran a guest. Worth knowing before this is called twice
in one process.

## State

`cargo test --workspace` green - **117 suites, 1984 tests**, 0 failures. clippy `--tests`
clean, fmt clean, identity scan clean.

Nothing committed. The day holds worklogs 292-336 and D466-D486.

**Next**: wire the link into the worker so a run actually gets the title's modules, then see
where PPSA02664 stops. After that the encoder path probes (48 outstanding), which the measured
537-module manifest makes answerable.
