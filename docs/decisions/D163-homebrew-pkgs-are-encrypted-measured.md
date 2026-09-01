# D163 - Homebrew pkgs are encrypted, measured; pkg support deferred


**decided** · 2026-08-20

Investigated whether downloadable homebrew could supply test material, and whether a pkg
could ever be launched directly. Both answers are more definite than expected, and both
are measurements rather than opinions - which is why they are here, so nobody repeats the
afternoon.

### Homebrew pkgs are encrypted, and the format says so

Two homebrew pkgs, from different authors using different packers:

| pkg | packer | PFS `mode` | encrypted | entropy |
|---|---|---|---|---|
| Apollo Save Tool | bucanero | `0x000d` | yes | 7.95 |
| Mono sample | marcussacana | `0x000d` | yes | 7.96 |

Bit 2 of the PFS superblock's mode field is the encrypted flag, and both set it. That is
the **container declaring itself**, not an inference from randomness - the entropy at the
theoretical ceiling of 8.0 and the total absence of `eboot.bin`, `sce_sys` or `param.sfo`
as plaintext merely corroborate what the header states.

So it is not a quirk of one tool. Fake-signing encrypts the filesystem regardless of who
built it, because that is what makes a pkg the console will mount.

**Therefore a pkg can never yield a runnable executable here.** Extracting one needs a key,
keys are outside this repository permanently (principle 1), and the provenance job fails
the build on them. Not a difficulty to engineer around - the constraint that keeps the
project distributable.

### What a pkg *can* give, and the shape support would take

The outer container is entirely plaintext: content id, `param.sfo`, icons, XML. Enough for
a proper library row - name, icon, version - which is more than the two pkgs in the local
library get today, being invisible.

The design, recorded so it need not be re-derived:

1. Read the metadata and show the pkg as a library row.
2. **Extract** where an extracted title shows **Start**, disabled with a reason until the
   user configures their own unpacking tool - which anyone with a jailbroken console
   already has.
3. Extraction lands in a cache directory and becomes an ordinary extracted title, running
   the path that works today.

Orbistoun never holds a key; it spawns a program the user supplies. Same contract as any
emulator that asks for user-provided dumps.

**Deferred** rather than built: it needs sign-off on spawning a user-configured external
program, which is a user-visible mechanism the log does not cover, and the payoff is a
nicer library rather than a guest that runs further.

### Downloadable payload ELFs exercise almost nothing

> **Corrected by [D305](#d305---a-plain-name-is-a-nid-nobody-hashed-yet) on 2026-08-26.**
> The measurement below is wrong for the builds in circulation: all twenty-three payloads
> re-tested carry `PT_DYNAMIC`, `DT_NEEDED` and a full named import table. What blocked
> orbistoun was `DT_GNU_HASH` and a path that silently discarded plainly named imports -
> not an absence of imports. **The conclusion drawn from it, that this route is closed,
> does not hold.** The paragraph stays as written because the reasoning from a false
> premise is the useful part of the record.

`ps5-payload-dev` publishes loose `.elf` payloads for both generations under GPL-3.0 -
`ftpsrv`, `klogsrv`, `gdbsrv`, `shsrv`, plus `websrv` carrying a catalogue of ported apps.
They looked ideal: plain ELFs, no container, open source, stable release tags, and the same
program built for both targets.

They parse - `ET_DYN`, x86-64, four program headers, recognised as bare ELF. And then:

```text
no usable dynamic table: no PT_DYNAMIC segment
```

**No dynamic segment at all.** Payloads resolve everything through the loader's own
function table at run time, so there is no import list, nothing for the NID resolver to
match, and nothing the HLE layer can intercept. They would load and then be invisible.

That reframes phase 0d. The value is not in *downloading* homebrew to run - it is in
**building** a guest with the open toolchain, which produces a real import table. Which is
exactly what obSCEne already is, and why it is the entry that matters in any corpus
manifest.

**Emulator projects were deliberately not opened.** shadPS4, GPCS4, KytyPS5 and obliteration
came up prominently in the search; principle 1 puts their source off-limits and none of it
was read.

