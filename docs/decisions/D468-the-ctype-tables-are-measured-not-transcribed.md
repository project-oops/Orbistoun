# D468 - The ctype tables are measured off hardware, because the documented layout is the wrong one

**measured** - 2026-09-02 (user-directed /loop: an obSCEne hardware capture arrived mid-session)

`_Getpctype` answers a pointer to the table `isalpha`, `isdigit` and the rest index, and the guest
*dereferences what it gets back* - so the `Unimplemented` placeholder is not a wrong answer that
propagates, it is an address read through (D459). Filling it in needs the table's actual contents.

[D448](D448-the-obscene-oracle-s-clean-orbistoun.md) recorded the plan: FreeBSD documents the ctype
table, that is the lawful oracle, transcribe it. **That would have produced a wrong table**, and this
is the entry that says so, because an obSCEne capture of the running console now exists to compare
against.

## The measured layout disagrees with the documented one

Derived from the capture by asking which characters carry which bit - not read out of anyone's source:

| bit | members | meaning |
|---|---|---|
| `0x01` | `0-9 A-F a-f` | hexadecimal digit |
| `0x02` | `A-Z` | upper |
| `0x04` | space | space |
| `0x08` | `!"#$%&'()*+,-./:;<=>?@[\]^_`{\|}~` | punctuation |
| `0x10` | `a-z` | lower |
| `0x20` | `0-9` | digit |
| `0x40` | HT LF VT FF CR | the whitespace controls |
| `0x80` | `\x00-\x1f`, `\x7f` | control |
| `0x400` | HT | tab alone |

FreeBSD's `ctype.h` numbers these entirely differently - `_CTYPE_A` is `0x100`, `_CTYPE_C` `0x200`,
`_CTYPE_D` `0x400`. **`0x400` is FreeBSD's digit bit and is the measured table's tab bit.** A
transcribed table would have made `isdigit` answer true for the tab and false for every digit, and it
would have done so silently, in a shader-free part of the emulator where nothing renders wrong enough
to notice. `crates/orbistoun-libc/src/ctype.rs` pins this in a test that names the trap.

The platform's C runtime is evidently not FreeBSD's libc here - the small-bit layout, and the
`_Getpctype`/`_Getptolower`/`_Getptoupper`/`_Mtx_init`/`_Xtime_get_ticks` naming, are a different
lineage. That is worth knowing beyond this table: **"FreeBSD-derived kernel" does not license assuming
a FreeBSD C library**, and the four-oracle list in `CLAUDE.md` should be read with that split in mind.

## The capture validates its own parse, and caught the first attempt

The probe records the table twice over: 34 rows of raw bytes, *and* a dozen spot-checks taken through
the running library (`mask_a_97 = 0x11`, `mask_eof_neg1 = 0x0`, ...). The generator refuses to emit a
table that disagrees with them, and three negative tests hold that guard down - a guard nobody has
watched reject something is a guard nobody knows anything about.

It rejected the first run. The record is named `table_raw_neg16` and **that sixteen is bytes, not
entries**: the margin below index zero is eight `u16` entries, not sixteen. Read as entries it put
index 0 half a table away, and every classification would have been wrong while the file still looked
entirely reasonable. The alignment was then solved rather than guessed - one offset satisfies all
twelve spot-checks at once - and the constant now says which unit it is in.

## Shape of the change

Extraction is a generator subcommand (`orbistoun-gen ctype`), beside `constants`, which reads an
external source and writes a crate's `data/` file; not a stand-alone script. Like `constants` it is
deliberately *not* in the `tables` verb, because that verb re-derives from in-repo recordings and this
reads a capture that lives in a sibling repository. The tables land in
`crates/orbistoun-libc/data/ctype.toml` carrying `known_by = "measured"`, and the three functions
allocate them into guest memory once and answer the address of entry zero - the *middle* of the
allocation, since `table[-1]` must be readable.

## What it did not do

**The wall did not move.** PPSA02664 still faults at `image+0xb14be3`, verdict `same`, six runs in six.
See worklog 297: the implementation binds (import slot 54) and is demonstrably called - 415 times in a
run, answering a real pointer - and yet the call at the faulting site still receives `0x7fff0001`.
Two routes to one symbol is the leading hypothesis and it is **not yet diagnosed**. This entry claims
a correct table and nothing about progress; D450 predicted the wall would move to the tlsf allocator
and it did not move at all, which is a third thing neither entry predicted.
