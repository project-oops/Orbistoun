# What downloadable homebrew can and cannot do for us


Scanned GitHub for open-source homebrew to use as test material, ahead of building the
corpus tooling. Two findings, both measured, both narrowing the plan rather than widening
it (D163).

**Homebrew pkgs are encrypted.** Two of them, different authors, different packers, both
with bit 2 of the PFS superblock mode set - the container declaring itself encrypted rather
than me inferring it from entropy at 7.95. No plaintext `eboot.bin`, `sce_sys` or
`param.sfo` anywhere inside. So a pkg cannot yield a runnable executable here, ever: that
needs a key, and keys are outside this repository permanently.

What a pkg *can* give is its outer container, which is entirely plaintext - content id,
`param.sfo`, icons. Enough for a real library row, which is more than the two pkgs sitting
in the local library get today. Support deferred, design recorded so it need not be
re-derived.

**Payload ELFs exercise almost nothing.** `ps5-payload-dev` publishes loose `.elf` payloads
for both generations, GPL-3.0, stable tags, and even the same program built for both
targets - which looked like a free differential on the API surface. They parse cleanly as
bare `ET_DYN` and then report `no PT_DYNAMIC segment`: payloads resolve everything through
the loader's own function table at run time, so there is no import list at all. They would
load and be invisible to every layer above the parser.

**So phase 0d is not "download homebrew to run".** The value is in *building* a guest with
the open toolchain, which produces a real import table - and that is precisely what obSCEne
already is. The manifest matters for pinning what we build and what obSCEne was built from,
not for harvesting other people's binaries.

Emulator projects came up prominently in the search - shadPS4, GPCS4, KytyPS5, obliteration
- and none of their source was opened. Principle 1.

