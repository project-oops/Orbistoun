# 564. Two builders wired, six refused, and the rule that decides which

**2026-09-14** - sweep 20260914-222710 answers six requests; the useful part is what separates them

## Wired

**`sceAgcDcbDrawIndex`.** The two body dwords worklog 536 could not place were reported. Called
`(3, 0x12345678, 0)` it writes `0xc0042700, 3, 0x12345678, 0, 3, 0` - the index count at **both**
body[0] and body[3], the third argument at body[4]. Twenty-four bytes, every one now read.

**`sceAgcDcbSetIndexSize`.** Swept over `type` 0-3 and `flags` 0-1: values `0x400, 0x440, 0x401,
0x441, 0x402, 0x442, 0x403, 0x443`, selector `0x20000243` throughout. So
`0x400 | (flags << 6) | type`. All eight points are pinned in a test; moving the shift to `<< 5`
fails it.

Ten builders wired, up from eight.

## Refused, and it is one rule rather than six judgements

| builder | measured | why not |
|---|---|---|
| `DcbAcquireMem` | header `0xc0065800`, 32 B, body **all zero** | called with every argument zero |
| `AcbWaitRegMem` | 56 B, three packets | zero arguments, and its Dcb twin is argument-sensitive |
| `CbDispatch` | 20 B | length only |
| `DcbDispatchIndirect` | 12 B | length only |
| `AcbDispatchIndirect` | 16 B | length only |
| `DcbDrawIndexOffset` | 20 B | length only |

**The rule: what decides implementability is not how much was measured but whether the arguments were
varied.** A length is not implementable. A packet captured at zero arguments is not implementable
either - `sceAgcDcbDmaData` has had a complete body since this morning and is still unwired for
precisely that reason. A packet across a few argument values usually is.

That is worth stating plainly because it looks like a quantity question and is not. `DcbAcquireMem`
now has *more* measured about it than `DcbSetIndexSize` did when it was refused, and is still
refused; `SetIndexSize` became implementable by adding eight cheap calls rather than any new depth.

`REQ-20260914T2154Z-72d7` asks for the same sweep on all six.

## Surprises

**The Acb and Dcb forms emit the same packet.** `sceAgcAcbWaitRegMem` produced 56 bytes byte-for-byte
identical to `sceAgcDcbWaitRegMem` at the same (zero) arguments. First evidence that the two rings'
builders share an encoding, which if it holds halves the remaining surface - and is one comparison
rather than a measurement, so it is recorded as an observation and not leaned on.

**I registered two handlers into a wrapped line and shipped them dead.** `rustfmt` had split the
registration entry across four lines, so an anchor matching the single-line form missed and both new
builders compiled, tested green as *encoders*, and were unreachable through `implementations()`. A
`never used` warning caught one. This is the second time in one day - worklog 543 found the same
shape in two AGC builders - so `tests/dcb_wiring.rs` now lists every wired name, and the reachability
test is what fails rather than a warning nobody reads.
