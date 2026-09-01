# 2026-08-19 (evening) - Names, and proving they are ours


The name search went from built to *working*, and then grew the provenance machinery it
needed. D070-D073; 24 crates, 265 tests.

**Two bugs and a milestone.**

The hasher had been reading the SHA-1 digest **little-endian since the first commit**.
Every NID this project ever computed was wrong. Nothing caught it because every test
hashed with an arbitrary suffix and compared against its own output - self-consistent,
and self-consistently wrong.

The suffix I had been treating as unobtainable was fine all along; the byte order was
the problem. Once fixed:

| Executable | Imports | Named |
|---|---:|---:|
| 2 MB | 715 | 83 |
| 96 MB | 1,380 | 161 |

And the function the 96 MB executable calls **431 million times** is
`sceKernelDirectMemoryQuery`. It asks about direct memory, is told "unimplemented", and
asks again forever. The wall has a name.

### Surprises

- **A test that compares a component to itself proves only internal consistency.** Both
  halves of a two-sided convention were individually well-tested and jointly wrong. The
  missing test was the *agreement*: `decode(encode(hash(name))) == hash(name)`, which
  needs no suffix, no module, and no external reference.
- **The combinatorial generator produced real vendor names.** Not just C library names -
  `sceSysmoduleLoadModule`, `sceSaveDataInitialize3`, `sceVideoOutSetBufferAttribute2`
  and dozens more, invented from a grammar and confirmed by arithmetic.
- **`{share:5.1}` on a `String` truncates it.** Precision on a string is a maximum
  length, so `99.9` printed as `9`. Looked like broken arithmetic.
- **The provenance question is better answered by reproducibility than by logging.** A
  log can be written after the fact; a re-derivation cannot. Verifying a recorded
  derivation costs an array lookup, which is what lets it gate every commit.
- **`observed` deserved to be its own category.** A name learned by debugging is
  entirely ours and yet not mechanically re-derivable. Folding it in with "came from
  outside" would have been unfair; folding it in with "generated" would have been a lie.

### Outstanding

**88% of imports are still unnamed.** That is a vocabulary problem, not a method
problem - `symbols/wanted.txt` is the work list and extending `data/vendor.toml` needs
no rebuild.

