# Harvesters that were designed and not built


Six mechanisms propose candidate names today and are described in
[PROVENANCE.md](../PROVENANCE.md). These were considered in the same pass and deliberately
left, with the reason, so they are not re-argued from scratch (D213).

- **Symbol-table residue** - *checked, and there is nothing to read.* An incompletely
  stripped module would leave real names in a `.symtab`, which would be the cheapest source
  on the list. Every module in the local corpus: no `.symtab`, no `.strtab`, five carrying
  a `.shstrtab` and nothing else. Worth re-checking if a corpus ever gains a title built
  differently; not worth writing code for this one.
- **Strings from mapped memory after relocation** - the whole image as the loader left it,
  rather than the file. Partly covered already by `argument-dump`, which reads the same
  memory but only where the guest handed a pointer to an import. The gap is text a guest
  produced and never passed anywhere - a decompressed table, a constructed path. Needs the
  worker to dump regions, which is a new artefact rather than a new question asked of one.
- **Call-position inference** - a function called once, first, and handed the entry point
  is a very small family. This is how the seventeen `call-trace` names were arrived at, by
  hand. It is a *targeting* mechanism rather than a naming one: it narrows which family a
  hash belongs to, and the grammar sweep is already cheap enough not to need narrowing. The
  version worth building is one that turns a profile into a **sub-grammar** - init verbs,
  attribute nouns - rather than one that ranks a worklist. Aimed squarely at
  `libkernel::0x6abac2f3dc6f8cee`, which is on the current wall.
- **Adjacent-error correlation** - a guest printing an error next to a call it just made
  names the function four times out of ten. The output exists; it goes to the worker's
  stderr, which is inherited and never captured, so there is nothing persisted to harvest.
  Capturing it belongs with the run report, not with the name search.
- **Cross-title argument correlation** - the same hash called with the same argument shape
  in two titles is the same function, which says nothing about its *name*. Useful for
  arity and for grouping, not for this.
- **A cited C++ ABI name list - measured, and it buys nothing. Not the highest-value item.**
  The seventeen `call-trace` names are `__cxa_*` and mangled C++ ABI symbols, all traceable to
  the Itanium C++ ABI, and that classification holds. What does not hold is the value.

  Three C++ vocabularies were run through `orbistoun-cli names` against the whole corpus -
  **2,932 unnamed imports across 54 modules** - and all three named **zero**: every identifier
  in GCC 15's `cxxabi.h` (659), the full export list of GNU `libstdc++.so.6` (6,046, of which
  6,000 are mangled `_Z*`), and that list transformed into libc++'s `std::__1` inline namespace
  (3,048), which is the form an LLVM target would actually use. Validated with a positive
  control: the same path reports `3 tried, 1 named` against an emptied symbol database, so the
  zeroes are measurements (worklog 595).

  **So the unnamed imports are not C++ standard-library or C++ ABI symbols** - a whole class
  eliminated, which is worth more than the re-citation this asked for.

  The provenance half does not work either: `--words-from supplied` records outside names at a
  *lower* tier than `call-trace` (D119), so that route moves the seventeen down. Doing it as
  `standard.txt` was done needs a machine-readable authoritative source, and the authoritative
  source is a prose specification. Leave it.

