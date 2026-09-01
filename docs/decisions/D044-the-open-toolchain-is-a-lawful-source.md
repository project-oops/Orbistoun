# D044 - The open toolchain is a lawful source for interface facts, never for implementations

**decided** · 2026-08-19

The [OpenOrbis Toolchain](https://github.com/OpenOrbis/OpenOrbis-PS4-Toolchain) is a
legal, open-source LLVM/Clang cross-compiler with headers, library stubs, startup
code, and linker scripts, built without vendor tools. Previous-generation only, which
fits the test-material decision already recorded in SCOPE.md.

Two uses, both legitimate, and one line between them:

- **Build test material.** Our own compiled apps are *real containers with clean
  provenance* - we wrote the source. That is better input for the parser than
  byte-crafted fixtures for the happy path, though synthetic fixtures are still
  needed for truncation, bad offsets, and malformed tables that a compiler will never
  emit.
- **Learn interface facts.** It is open source, so reading it is ordinary engineering
  under D024. It can seed D025's symbol name list without brute-forcing, and give
  real values for the 37 provisional arities.

**The line:** never let its *implementations* inform ours. Signatures and names are
interface facts; behaviour is not. Credited in ACKNOWLEDGEMENTS.md either way.

