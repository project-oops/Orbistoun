# 2026-09-01 - (/loop) The reserve-then-map fix advanced three titles, not one

Re-ran the corpus against the D460 binary to measure the fix's reach. The reserve-then-map bug
was not PPSA02664's alone:

- **PPSA02664**: 234 -> 1544 calls, FURTHER. New wall `image+0xb14be3` (`_Getpctype`).
- **PPSA03416**: +1307 calls, FURTHER. New wall `image+0xb14be3` (`_Getpctype`) - the same one.
- **PPSA25872**: 1755 -> 26850 calls (+25095), MIXED (more reached, different path). New wall
  `image+0x7b594e`.
- **PPSA04263**: unchanged, 32 calls, `image+0x2ba64c1` - dies early on its own bug (a null-base
  write after CreateSema), not the allocator.
- **PPSA21564**: unchanged, ~500k calls, `image+0x11ccd` - already past the allocator, walled on
  its `int 0x41` assert.
- **PPSA28061**: unchanged, 961 calls - GPU.

So one allocator-semantics fix moved three of the five remaining titles, and two of them now sit
on the *same* wall. That makes `_Getpctype` the highest-value crack left - it unblocks PPSA02664
and PPSA03416 together.

`_Getpctype` is oracle-gated, though, and must not be guessed. It returns a pointer to a
character-classification table the guest indexes inline (`table[c] & MASK`); orbistoun's libc
implements the ctype calls as predicates (`isalpha = is_ascii_alphabetic`), so no table exists to
point at, and the **bit-mask values** the guest ANDs against are the PS5 CRT's own (Dinkumware
`_UPPER`/`_LOWER`/... lineage). A table built with the wrong masks classifies every character
plausibly and wrongly - the exact honest-failure trap (principle 3) - so the masks have to come
from an oracle: the SDK `<cctype>`/`<yvals.h>` header, or an obSCEne probe reading the real table
off hardware. Recorded here rather than acted on, pending that input.

Next autonomous, oracle-free step: investigate PPSA25872's freshly exposed `image+0x7b594e` with
the return column - it may be another local-logic wall like the map one.
