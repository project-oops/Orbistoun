# 588. The family was five of eight and nothing said so

**2026-09-15** - orbistoun-shader, orbistoun-translate, after worklog 587

MTBUF has eight opcodes: load and store, at one, two, three and four channels. The tables
had five. The three missing ones were not refused, not listed, not counted - a shader using
one decoded as a bare family and opcode number, which is honest and is also invisible.

```
187 of 187 instructions translatable   (was 184 of 184)
MTBUF 0-7, eight of eight
```

## 1. Why nothing noticed

Because every count was of what the tables *have*. The census reports instructions seen in
the corpus, and the corpus is generated from the fixtures, and the fixtures only contain
instructions somebody wrote down. A family with three forms missing reports 100% and means
it - of what it was asked about.

**The gap surfaced from the other end.** Worklog 587 needed a three-channel typed load to
reach the narrow-packed-float refusal, found the mnemonic was not decodable, and wrote that
down rather than working around it. That note is what turned an absence into a task.

## 2. Measured, not transcribed

The opcode numbers were not read off a document. `tools/shader-fixtures/probes/typed.s`
gained five samples of each new form and the operand solver ran over the whole probe set in
the generator VM:

```
93 opcodes probed, 92 solved
```

The samples follow the probe file's own rules, which exist because four of the five opcodes
there failed on the first run for reasons the header now explains at length: the data
register reaches above 128 so the resource field cannot be read out of its low bits, the
resource group index is a mix of odd and even, and one sample per opcode uses a literal `0`
for the scalar offset so the field solves eight bits wide instead of seven. Following an
existing file's stated constraints is cheaper than rediscovering them.

The mnemonics came from `unreached.s` - the hand-written fixture that exists precisely
because no LLVM IR makes the compiler emit MTBUF - and the reference disassembler named
them. Its fixture grew from 104 to 128 bytes, three instructions of eight.

## 3. A recording that was stale and harmless

Re-recording the transcripts turned up that `operands-memory.in` had not been re-recorded
since the flat-offset probes were added (worklog 565). The gate was green anyway, because
the stale recording still solved the same table - so it was a latent hazard rather than a
live fault. It is current now.

Worth noting for what it says about the gate: **it checks that the table matches its
recording, not that the recording matches the probes.** A probe edited without re-recording
is caught only when it would change a solved layout. That is a real limit and this is the
first time it has been seen from the inside.

## 4. The refusal that could not be tested, and now is

`BUF_FMT_10_11_11_FLOAT` is three floats of ten and eleven bits, which are not IEEE halves.
It has been refused all along, and **it had no test**: asking for three components needs a
three-channel instruction, and there wasn't one. `a_packed_float_narrower_than_a_half_is_refused`
exists now.

That is the whole shape of this entry. A missing decode made a refusal unreachable, an
unreachable refusal made a gap invisible, and the count said 100% the entire time.

## 5. Also: the sync script was filling the VM's disk

`tools/toolchain/sync.sh push` extracted its archive in the guest and left it there - most
of a gigabyte, on an eleven-gigabyte disk that also holds a Rust toolchain and a `target/`.
The first symptom was a release build starting with 337MB free. It removes the archive after
extracting now.

## 6. Files

- `tools/shader-fixtures/probes/typed.s` - fifteen new probe samples.
- `tools/shader-fixtures/unreached.s` - the three forms, assembled and named.
- `crates/orbistoun-shader/data/{mnemonics,opcode-operands}.toml` and
  `crates/orbistoun-gen/tests/fixtures/transcripts/` - regenerated.
- `crates/orbistoun-translate/src/model.rs` - the three names in `SUPPORTED`.
- `crates/orbistoun-translate/tests/execute.rs` - the narrow-float refusal test.
- `tools/toolchain/sync.sh` - the archive removed in the guest.

## Next

1. Whether any other family is short of its forms the same way. MTBUF was found by
   accident; a check that compares each family's *named* opcodes against what the reference
   will assemble would find the rest on purpose.
2. `REQ-20260914T2348Z-4e71` and `REQ-20260914T1720Z-9c4a`, both still open on the obSCEne
   bus.
