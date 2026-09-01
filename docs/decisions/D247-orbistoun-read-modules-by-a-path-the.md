# D247 - orbistoun read modules by a path the console does not use


**decided** · 2026-08-25 · found in the first hour of pointing the conformance probe at the loader

`DynamicInfo::parse` read the **standard** dynamic tags - `DT_STRTAB`, `DT_SYMTAB`,
`DT_HASH`, `DT_RELA`, `DT_JMPREL` - and knew exactly two vendor tags, both for import
libraries. A console loader **ignores every standard tag** and reads `DT_SCE_*` entries
describing tables inside a `PT_SCE_DYNLIBDATA` segment. That is obSCEne's derivation, and it
is arithmetic rather than pattern-matching: the table offsets sum end-to-end.

**Every title in the local corpus carries both sets.** That is the only reason this has ever
worked. orbistoun was reading real material by a path the platform does not take, and would
have gone on doing so indefinitely - six commercial executables, none of which could expose
it, because none of them is built the way the platform expects.

A module that carries **only** vendor tags was refused outright: *"dynamic table lacks a
string table, symbol table, or hash table"*, said of a module that has all three.

### Three sites, one difference, and only one of them knew

A standard tag holds a **virtual address**. A vendor tag holds an **offset into the data
segment**. Resolving one as the other does not fail - it lands at a plausible file offset
holding the wrong bytes.

Three places resolved dynamic tables and each had to learn it:

| site | symptom before |
|---|---|
| `raw_imports` | "dynamic table lacks a string table" |
| `relocate::apply` | "2 relocations, 2 unsupported" - of two ordinary `GLOB_DAT`/`JUMP_SLOT` |
| `symbol_count` | **silently returned 0** - an empty thunk table, and every relocation blaming its symbol |

The third is the one worth remembering. It answers `0` rather than an error, so the failure
surfaced two layers away as "2 unresolved" against a symbol that was present and correctly
named. One method on the container now, used by all three.

### Zero is a value here, not a sentinel

The first attempt still refused the module. `is_usable()` tests `strtab != 0`, which is
right for a virtual address and wrong for an offset - **the probe's string table is at
offset 0**, the first byte of the data segment.

So the vendor tags are collected as `Option`, and presence is what the parser *saw* rather
than a test on the value. This is the third time today the same trap has appeared: a real
value spelled the same as absence. It was an empty string standing for "the record did not
say" (D245), a zero count standing for "the diagnostic did not run" (D241), and now offset
zero standing for "no table".

### What it cost and what it bought

Twenty minutes and one build of a 126-line module with a single import. Three defects, each
isolated by a one-line signal, in sequence. With the full probe's four hundred imports
across fourteen libraries, every one of them would have surfaced together and been read as
"the probe does not load".

The minimal module is a bisection instrument, not a lowered bar. Nothing fixed for it was
specific to it: all three fixes are how the platform reads any module, including the six
titles that were quietly relying on carrying both sets of tags.

