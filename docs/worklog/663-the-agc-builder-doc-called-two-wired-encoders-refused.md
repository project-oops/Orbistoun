# 663. The AGC builder doc called two wired encoders "refused" - corrected, and 4059 closes

**2026-09-17** - `agc.rs` had a stale refusal note claiming `sceAgcDcbDrawIndex` and
`sceAgcDcbSetIndexSize` were "deliberately not wired", while both sit in `implementations()` as
measured encoders; correcting it shows the corpus-reached builders are all wired, closing inbox
`-4059`

## The contradiction

Two doc sites in `agc.rs` - the module doc and the block just above `implementations()` - said the
two builders worklog 536 first refused were still "deliberately not wired", each measured on only one
input. The code says otherwise:

- `dcb_draw_index` (in `implementations()`, line 884) calls `packet::build::draw_index_2` - the two
  body dwords worklog 536 could not place were read back on a later capture (`(3, 0x12345678, 0)`
  wrote `0xc0042700, 3, 0x12345678, 0, 3, 0`), and the encoder places them.
- `dcb_set_index_size` (line 885) is "measured across eight argument pairs, which is what makes it
  implementable where worklog 536 refused it on one" - its own function doc already said so.

So the refusal note named two things no longer refused - the same anti-pattern the codebase calls out
elsewhere (a refusal that guards nothing). Both doc sites now say the two were later measured across
more inputs and wired.

## Why it closes 4059

Inbox `-4059` asked for the builders the corpus reaches, each implemented with a test or refused with
its measurement filed. Its own analysis named exactly two reached-but-unimplemented ones -
`sceAgcDcbDrawIndex` and `sceAgcDcbSetIndexSize` - plus the title-walling
`sceAgcDcbSetCxRegistersIndirect`, since done from obSCEne `-4386` (worklog 616). All three are now
wired as measured encoders (or, for the indirect, a measured reservation skeleton returning a real
cursor), pinned by `tests/dcb_wiring.rs`. The stale doc was the only thing still saying the coverage
had gaps. So `-4059`'s reached-builder coverage is met, and it is resolved.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets` clean; `tests/dcb_wiring` 15 passed; fmt clean; identity
scan exit 0. Doc-only change to `agc.rs`. No commit.
