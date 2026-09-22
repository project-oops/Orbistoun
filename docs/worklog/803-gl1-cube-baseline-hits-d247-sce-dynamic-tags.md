# 803. The gl1-cube baseline's first run through orbistoun hits the D247 loader gap: a properly-built module carries only the SCE dynamic tags, and orbistoun's linker reads the standard ones the retail corpus happens to also carry

**2026-09-22** — the operator's direction (backlog 037): oops-gl/oops-mesa now render 3D on native
hardware, and their cube demos are orbistoun's **own source end to end**, so they are the baseline the
tool-bound retail frontier lacks. This is the first run of one through orbistoun, and it paid off
immediately - a real, buildable orbistoun gap the retail titles had masked.

## gl1-cube is in the corpus and maps, but does not link

The gl1-cube title package (`GLCB00001`: eboot, `libc.prx`, `sce_sys`) unzipped into the data corpus and
ran. It reaches `Mapped` - two segments placed, 1,031,888 bytes copied at `0x400000000000` - and then:

```text
could not link the title: container: no usable dynamic table:
dynamic table lacks a string table, symbol table, or hash table
```

## The cause is D247, and orbistoun's own source names it

`crates/orbistoun-elf/src/dynamic.rs:88-100` documents exactly this: a console loader **ignores**
`DT_STRTAB` / `DT_SYMTAB` / `DT_HASH` and reads the **SCE tags** instead - `SCE_HASH = 0x61000025`,
`SCE_STRTAB = 0x61000035`, `SCE_SYMTAB = 0x61000039` - whose values are **offsets into
`PT_SCE_DYNLIBDATA`**, not virtual addresses. And: *"Every title in the local corpus happens to carry the
standard tags too, which is why reading only those has worked. A module built the way the platform
expects carries only these - the conformance probe's minimal module does, and orbistoun refused it with
'dynamic table lacks a string table, symbol table, or hash table' while the module had all three."*

gl1-cube's dynamic table (read from inside `PT_SCE_DYNLIBDATA`, where its `DYNAMIC` segment lives - it has
no wrapper block of its own) carries the SCE tags `0x61000025/35/37/39/3b` and **not** the standard
`DT_STRTAB/SYMTAB/HASH`. So it is exactly the D247 case: a module built the platform's way, which the
retail corpus never was, and orbistoun's linker cannot see its tables.

This is the baseline doing its job. The retail titles are all reverse engineering and carry the redundant
standard tags by construction; the first fully-owned guest, built by our own SDK the way the platform
expects, is the one that exposes that orbistoun reads the wrong half of the dynamic table.

## Next: resolve strtab/symtab/hash from the SCE tags

The fix is in orbistoun, not the SDK (the SDK builds a conformant module; orbistoun reads it wrong). The
SCE tag constants already exist in `dynamic.rs`; what is missing is using them: parse `SCE_STRTAB/SYMTAB/
HASH/STRSZ/SYMENT` into the dynamic `Info`, resolve them as offsets into `PT_SCE_DYNLIBDATA`, and prefer
them over the standard tags (or fall back to them when the standard ones are absent), so the "lacks a
string table" refusal (`lib.rs:593`) is reached only when *neither* pair is present. Then gl1-cube links
and the run moves past `Mapped` for the first time - the next observation the baseline gives.

## Gate state

No repo code changed - gl1-cube added to the data corpus (not the tree) and its first run characterised
to the D247 SCE-dynamic-tag gap, with the fix scoped in orbistoun's linker. `./bin/orbistoun check` green,
worklog index regenerated, identity scan clean. No commit.
