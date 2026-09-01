# D049 - Containers are wrapped; the ELF is not at offset zero

**decided** · 2026-08-19 · observed from real material

`orbistoun-elf` was written expecting `\x7fELF` at offset 0. Real containers are not
shaped that way, and would all have been rejected.

**Observed**, consistently across a commercial executable and three modules it ships:

| | |
|---|---|
| Outer magic | `54 14 f5 ee` |
| Inner ELF begins at | offset **416** |
| `EI_CLASS` / `EI_DATA` | 64-bit, little-endian |
| `EI_OSABI` | **9 - `ELFOSABI_FREEBSD`** |
| `e_machine` | `0x3E` - x86-64 |
| `e_type` | `0xFE10` executable, `0xFE18` module |

Three things follow.

**The offset must be read from the wrapper header, never hardcoded.** 416 held for
every file inspected, and the header is consistent with a 32-byte prologue followed by
twelve 32-byte entries (`0c 00` at offset `0x18` reads as a count of 12; 32 + 12×32 =
416 exactly). That is a *hypothesis fitting the observations*, not a specification -
D010 applies, so the parser derives the offset rather than assuming it.

**`EI_OSABI = 9` states the FreeBSD lineage in the binary itself**, rather than by
inference. That is the foundation of oracle #1 in [TESTING.md](../TESTING.md), now
confirmed from primary evidence.

**The generation split is visible inside one title.** The bundled `sce_module/*.prx`
carry the current-generation magic above; substituted `fakelib/*.sprx` in the same
directory carry `4f 15 3d 1d`, the previous generation's. Two wrapper formats will
have to coexist eventually - but current-generation is what to parse first, since it
is what real material uses.

Amends D012: individual `e_type` values were held unasserted pending verification.
Two are now observed. That is evidence, not a complete enumeration - record what has
been seen, keep rejecting what has not.

