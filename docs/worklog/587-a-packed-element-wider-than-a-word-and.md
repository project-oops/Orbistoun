# 587. A packed element wider than a word, and the straddle nobody has to guess at

**2026-09-15** - orbistoun-translate, after worklog 586

The `16_16_16_16` family translates - all seven codes. It is the first typed element wider
than a dword that is not simply four independent words, and the thing that made it small was
noticing which half of it is arithmetic.

## 1. The part that is arithmetic

An element is a **little-endian byte sequence with component zero first**. That is not a
supposition about the hardware; it is what the already-checked cases mean. `8_8_8_8` puts
component zero in the low byte and `16_16` puts it in the low half, both measured on a
device. So the component at bit `b` is in word `b / 32` at bit `b % 32`, because word `w` is
bytes `4w..4w+4` and bit 32 of the element *is* word one's bit zero.

Nothing had to be decided. The extraction the single-word path already does is the same
extraction, indexed.

## 2. The part that is not, and is refused

A component that **spans a word boundary** would have to be assembled from two reads, and
which end goes where is a rule nothing here has measured. No format in the table does it -
every width divides the word it sits in - so the check refuses a case that does not arise
rather than guessing at one that might:

```rust
let crosses = bit / 32 != (bit + width - 1) / 32;
```

Written as a computed property rather than a list of safe formats, so a format added to the
table later is covered without anybody remembering to look.

## 3. One check, not one per word

The element is bounds-checked **once, whole**, and then read a word at a time. That is the
difference from the untyped path, which checks each word separately because there each word
*is* an independent component. Here the words are one value, and a check per word would let
half an element be read at the very end of a buffer - which is exactly the case the check
exists for.

## 4. The refusal test moved, and that is the point

`a_typed_buffer_format_needing_conversion_is_refused_by_name` has now moved twice: off the
single-word normalised formats when those landed, and off `16_16_16_16_UNORM` today. **A
refusal test naming something no longer refused passes while guarding nothing**, which is the
quiet version of this log's recurring failure.

It sits on a packed **store** now, which is the largest thing still refused that a decodable
instruction can express. Packing a value back down is not unpacking it backwards: a store has
to choose a rounding, saturate what will not fit, and decide what happens to bits the
components do not cover, none of which is measured.

**And there is a refusal with no test at all, which is worth writing down rather than
working around.** The 11- and 10-bit packed floats are refused, and reaching one needs a
three-channel typed mnemonic - `tbuffer_load_format_xyz`. The mnemonic table has the one-,
two- and four-channel forms and not that one, because the table is generated from what was
measured and nothing has measured it. Adding a row by hand to make a test reachable is the
opposite of what the table is for.

## 5. What is left

| refused | why |
|---|---|
| 11- and 10-bit packed floats | not IEEE halves; the decode is unmeasured |
| `SRGB` | a piecewise transfer curve, not arithmetic on the field |
| packed stores | rounding, saturation and the uncovered bits, all unmeasured |
| components spanning a word | does not arise; would need a rule nobody has |

`SRGB` never appears in the measured table at all, so its arm is unreachable through any
format code that exists. It is named explicitly anyway - clippy asked for it, and the
exhaustive match means a kind added later cannot fall silently into a wildcard.

## 6. Files

- `crates/orbistoun-translate/src/model.rs` - `packed_format_admitted`'s straddle check and
  `ELEMENT_BITS`; the per-word read in `packed_buffer_memory`.
- `crates/orbistoun-translate/tests/execute.rs` - the two-word device test, and the refusal
  test repointed at the store.

## Next

1. A three-channel typed mnemonic, measured, which would make the narrow-float refusal
   testable - and is the only route to it.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both still open on the obSCEne
   bus.
