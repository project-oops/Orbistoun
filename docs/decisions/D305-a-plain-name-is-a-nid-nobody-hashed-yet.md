# D305 - A plain name is a NID nobody hashed yet


**decided** · 2026-08-26

D163 recorded that the `ps5-payload-dev` payloads have **no dynamic segment at all**, so
"there is no import list, nothing for the NID resolver to match, and nothing the HLE layer
can intercept". On that basis the whole route was closed.

**Measured again against twenty-three payload ELFs and it is false.** Every one carries
`PT_DYNAMIC`, `DT_NEEDED`, a real `.dynsym`, and between 3 and 207 named undefined symbols.
Either the August test hit older builds or the previous-generation variants; these link
against `libkernel_web.sprx`, `libSceLibcInternal.sprx` and `libSceNet.sprx` and say so in
their own tables. orbistoun's refusal had already changed accordingly - from "no PT_DYNAMIC
segment" to "lacks a string table, symbol table, or hash table" - and nobody read it.

Two things actually blocked it, and one of them was a silent wrong answer.

### The count `DT_GNU_HASH` does not state

Twenty-one of the twenty-three carry **only** `DT_GNU_HASH`. The symbol count comes from
`DT_HASH`'s `nchain`, and there is no equivalent word here - the count has to be walked out
of the bucket array and the chain's stop bits. `DT_HASH` is still preferred where a module
carries both: it states the answer, and a stated answer cannot be walked wrong.

### The silent one: plain names were dropped

`imports_from_symbols` ended in `if let Some(decoded) = decode_symbol_name(name)`, and a
name that did not parse as `NID#lib#mod` was skipped without a word. So the two payloads
carrying `DT_HASH` **got past the first gate and reported `0 imports, 0 unresolved`** - a
module needing eighty-five things reporting that it needs none.

That is the exact claim principle 3 forbids an import list from making, and the guard that
exists for it (`Container::imports` errors rather than returning an empty list) did not
cover this path. A guard nobody has watched reject something is a guard nobody knows
anything about.

### Why hashing the name is the native answer and not a second scheme

The tempting reading is that these need a *second* resolver: NIDs for vendor modules,
strings for homebrew. They do not. `libSceNet.sprx` exports `socket`, and the NID it
publishes **is** `SHA-1("socket" + suffix)`. A plain name is not a different kind of import.
It is a NID nobody hashed yet.

So `RawImport` now always carries a NID, computed where it was not encoded, and everything
downstream resolves one way. **Checked before it was built**, because the whole design rests
on it: `socket`, `bind`, `listen`, `accept`, `malloc`, `pthread_create`, `memcpy`, `sysctl`,
`kqueue` and `getifaddrs` are all in this repository's own hash-confirmed symbol database,
which means each already hashes to a NID that appears in real vendor modules. The relation
was established by the naming loop years of NIDs ago; this only stops discarding it.

`raw_imports` therefore takes the hasher rather than making one. The registry's own hasher
is what callers pass, because a name hashed with a different suffix resolves to nothing and
does so **silently**, as an unresolved import rather than as an error.

### What it does not carry

A vendor-encoded name carries its own attribution; a standard SysV name carries none. So
`library_id` and `module_id` become one `NameForm` enum and the accessors return `Option`.
`None` is "the format does not record this", not "library zero" - blending the two would
attribute every homebrew import to whichever library happened to be first.

### Result

| payload | imports | orbistoun can name | registry answers |
|---|---|---|---|
| elfldr 0.25 | 24 | 24 | 8 |
| klogsrv 0.9 | 34 | 33 | 7 |
| shsrv 0.20 | 41 | 39 | 10 |
| ftpsrv 0.21.1 | 85 | 84 | 24 |
| pldmgr 0.5.1 | 160 | 159 | 42 |

**Cross-checked against `readelf`**, which counts distinct undefined dynamic symbols
independently: it agrees on all five, exactly. That is the useful kind of confirmation -
an external tool that has never heard of this project arriving at the same number - and it
caught a first pass that was one too high on every row, from counting a trailing blank
line as an import.

**The naming problem was already solved for these.** One or two imports per payload are
unnamed; the rest were nameable the moment the loader stopped throwing them away. What is
left is implementation, which is a different and more ordinary kind of work.

Vendor modules are unaffected and were checked: a title's eboot still attributes to
`libkernel` with its ids intact, and the conformance probe's own module still reads.

