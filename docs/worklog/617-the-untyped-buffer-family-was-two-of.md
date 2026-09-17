# 617. The untyped buffer family was two of eight, found the same way MTBUF was

**2026-09-16** - orbistoun-shader, orbistoun-translate, the finding filed as `REQ-20260915T0929Z-6ec2`

MUBUF has eight forms a shader uses to move raw dwords: load and store, at one, two, three and
four dwords. The tables had two - `buffer_load_dword` and `buffer_store_dword`. The other six
decoded as a bare family and opcode number, and the census reported 100% the whole time, because
it counts only instructions the tables name. This is exactly the shape worklog 588 found in MTBUF,
and it was the finding filed right after.

```
census: 193 of 193 instructions (+6), no blockers
MUBUF opcodes named: 12,28  ->  12,13,14,15,28,29,30,31
```

## Measured, not transcribed

The opcodes are not contiguous and were not guessed: llvm-mc in the build VM assembled each form
and the bytes gave load x2=13, x4=14, x3=15 and store x2=29, x4=30, x3=31 (the sanity check that
`buffer_load_dword` still reads as 12 and the store as 28 confirmed the extraction). The mnemonic
table then came from disassembling the six forms added to `unreached.s`, and the operand layouts
from probe samples added to `probes/memory.s` and run through the solver - the same two-generator
path as 588. `./bin/orbistoun tables` is green, so the committed tables still reproduce from their
recordings.

## The solver refused three at first, for a tie the probe file already documents

The first run solved three forms and refused the other three:

```
unsolved: MUBUF:0xf (buffer_load_dwordx3), 0x1d (buffer_store_dwordx2), 0x1e (buffer_store_dwordx4)
```

All three of my refused sample sets happened to use **consecutive odd resource group indices**.
`memory.s`'s own flat-memory note explains why that cannot solve: the group index's low bit is bit
16, and with every index odd that bit is always set, so a nine-bit window over the data field
explains every sample as well as the true layout does - the solver sees two readings it cannot
separate and correctly refuses rather than pick. The three that solved had even or mixed groups.

The fix was to mix odd and even groups in the three refused sets; all eight then solved (the
`s_waitcnt` skip is pre-existing and unrelated). The lesson is now written into the probe comment,
because it is the third time a variant of this exact tie has bitten and the file is where the next
person will look.

## The translation was already written

`buffer_access` has taken a component count since the typed forms landed - it moves N consecutive
dwords to N consecutive registers with a bounds check per dword. So the only new code is reading
the width from the name (`dword` -> 1, `dwordx2/3/4` -> 2/3/4) in `buffer_memory`, and letting the
dispatch reach it for any `buffer_` name (a `tbuffer_` name does not start with `buffer_`, so the
two families stay distinct). A width the name does not carry is refused, not guessed.

## Made to fail

`a_four_word_untyped_access_moves_four_consecutive_words` stores four distinct words through
`buffer_store_dwordx4` and reads them back through `buffer_load_dwordx4` on a device, comparing
against *memory* so a translation that collapsed the four onto one address cannot pass a
register-to-register check instead. Forcing the x4 count to 1 made it fail with three zero words,
which is the failure it exists to catch; reverted.

## Files

- `tools/shader-fixtures/probes/memory.s` - the probe samples, and the odd/even note.
- `tools/shader-fixtures/unreached.s` - the six forms, hand-assembled for the mnemonic table.
- `crates/orbistoun-shader/data/{mnemonics,opcode-operands}.toml`,
  `crates/orbistoun-gen/tests/fixtures/{unreached.*,transcripts/}` - regenerated.
- `crates/orbistoun-translate/src/model.rs` - the width read, the collapsed dispatch arm, the eight
  names in `SUPPORTED`.
- `crates/orbistoun-translate/tests/execute.rs` - the device test.

## Next

The census names no remaining unsupported instruction in the corpus. What is left in G10 is the
*host* side - a descriptor base reaching real host memory - which is capture-shaped
(`REQ-20260915T0929Z-31de`), and the standing MTBUF refusals, each waiting on a measurement
(`REQ-20260915T0929Z-4c2e`).
