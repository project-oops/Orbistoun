# 2026-08-19 - Real material arrived; plan revised against it


`titles/` created and populated with a commercial title (~96 GB). Verified invisible
to git: dummy `eboot.bin`, `.pkg`, `.prx`, `.self`, a 2 MB binary, nested paths and a
path containing a space - all ignored; only `titles/README.md` tracked.

**A hole found by testing rather than assuming.** The provenance guard did *not* catch
a `git add -f`ed guest binary, because `titles/` had been added to its exemption list.
That was backwards - the guard inspects the index, corpus content is gitignored, so
anything appearing under `titles/` is exactly the failure worth catching. The
exemption made the second line of defence blind to the directory it most needs to
watch. Removed from all three enforcement points; re-tested, guard now fails with exit
1. D042 amended to record why the original reasoning was wrong.

**Findings from the material itself, all in D049.** The single most important:
**containers are not plain ELFs.** Outer magic `54 14 f5 ee`, inner ELF at offset 416,
consistently across the executable and three bundled modules. `orbistoun-elf` as
written expects `\x7fELF` at offset 0 and would have rejected every real file. Also
observed: `EI_OSABI = 9` (`ELFOSABI_FREEBSD`) stated in the binary rather than
inferred, `e_type` `0xFE10`/`0xFE18`, and both generations' wrapper formats coexisting
inside one title.

**Surprises.**
- **Two planning assumptions were invalidated at once** (D050). Phase 0's whole
  rationale - "no real container can ever live here, so synthesise one" - became
  partly obsolete the moment real material existed *outside* the repo. And obSCEne's
  early justification (being our first real container) evaporated with it, while the
  previous-generation toolchain means it would emit the wrong wrapper format anyway.
- **The best first parser target is a 76 KB bundled module**, not the 68 MB
  executable. Same format, same wrapper, trivially inspectable - and it was sitting in
  the same directory the whole time.
- **Title identity must hash the executable, not the directory.** 96 GB per run is a
  non-starter; the executable is also the semantically right thing, since it is what
  changes when a title is patched.

**Next.** 0c, then 0e, then phase 1 against real material, with phase 0 alongside.

