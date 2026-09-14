# 534. Ten AGC builders measured as whole packets, and a specific prosper claim refuted

**2026-09-14** - second pass over obSCEne sweep 20260914-100833, after worklog 533

## What worklog 533 missed

533 recorded fourteen packet **sizes** and said the bodies were unmeasured. That was wrong about the
same sweep: ten more builders had been called and had reported a **packet header and body**, not just
a size. The reason I missed them is worth writing down, because it is a search error anyone repeats -
the size checks report through `OBS|bytes`, these report through `OBS|measure`, and a grep for the
first finds none of the second. Counting the wrong record type produced "zero captured" for a section
that had captured ten.

## The ten packets

Every one decodes as a type-3 PM4 packet whose count field closes against the byte count the builder
advanced - `(count + 2) * 4` equals the advance, on all ten. That is the field split D565 derived from
four builders and could not then confirm; it is confirmed now.

| builder | bytes | dwords | header | body as measured |
|---|---|---|---|---|
| `sceAgcDcbEventWrite` | 8 | 2 | `0xc0004600` | event type `0x3e` |
| `sceAgcDcbSetIndexCount` | 8 | 2 | `0xc0001300` | count `0x24` |
| `sceAgcDcbSetNumInstances` | 8 | 2 | `0xc0002f00` | instances `1` |
| `sceAgcDcbDrawIndexAuto` | 12 | 3 | `0xc0012d00` | count `3`, initiator `2` |
| `sceAgcDcbSetIndexBuffer` | 12 | 3 | `0xc0012600` | address lo/hi |
| `sceAgcDcbSetIndexSize` | 12 | 3 | `0xc0017a00` | selector `0x20000243`, value `0x400` |
| `sceAgcDcbSetCxRegisterDirect` | 12 | 3 | `0xc0016900` | register `0x200`, value |
| `sceAgcDcbSetUcRegisterDirect` | 12 | 3 | `0xc0017900` | register `0x242`, value `4` |
| `sceAgcCbSetShRegisterRangeDirect` | 16 | 4 | `0xc0027600` | register `8`, two values |
| `sceAgcDcbDrawIndex` | 24 | 6 | `0xc0042700` | address lo/hi, count (3 of 5 body dwords reported) |

The probe's own arguments come back in the body unchanged and in order, so the argument-to-dword
mapping is direct for every one of them. The knowledge base is now 37 AGC entries, 33 `measured`.

## Against prosper: five agree, four differ, one claim is refuted

The divergences have a shape, and it is not "they misread the hardware":

- **`CbSetShRegisterRangeDirect` - a specific claim, refuted.** Their table gives `n + 4` dwords and
  states the builder prepends a two-dword NOP marker `0x6875000d`, described there as mirroring the
  real library. The measured packet begins with the register write itself and carries **no marker of
  any kind**. The real library emits `n + 2`. This is the first time a measurement here has contradicted
  a named mechanism rather than a number.
- **`DcbDrawIndexAuto` 3 vs 7, `DcbDrawIndex` 6 vs 7** - both are their own private payload (their
  table says a 64-bit draw modifier), the same shape as their `DcbJump` +1 in worklog 533.
- **`DcbSetIndexSize` 3 vs 2** - the only row where they are **short**, and the opposite direction
  from every other divergence found.
- **`DcbEventWrite` 2 vs 4 is not a divergence.** The call measured here passed no address, and their
  row is explicitly the address-carrying form. Two forms of one packet. Recorded as such, with a
  warning not to encode 2 as the only shape.

**The pattern worth keeping:** on every row where a published PM4 size exists and prosper differs, the
published size matches the hardware and prosper does not. Their figures are honest about their own
emitter; they are not a description of the library. That is exactly what D683 assumed and is now
evidence for it rather than reasoning about it.

## Surprises

**A quoting failure cost a turn.** The batch was first written as one large heredoc and the command
was truncated mid-string, which surfaced as an unbalanced-quote error rather than as a length error.
Splitting into four short commands worked immediately. Long generated shell is worth avoiding for the
same reason long generated anything is.

**oops-sdk agrees by hand.** Its `src/agc/agc_draw.c` writes `0xc0002f00`, `0xc0016900` and
`0xc0017900` - with register `0x242` for the last - derived from the public hardware documentation and
never from the library. Three of the measured headers match it exactly, register offset included. Told
them so on their own bus; five of their literals are now corroborated rather than assumed.
