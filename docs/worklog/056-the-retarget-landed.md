# The retarget landed


RDNA2, `gfx1030`, 64-lane waves. Every suite green except one device test, described at
the end.

The family rows came from the published reference - *"RDNA 2" Instruction Set
Architecture: Reference Guide*, AMD document 70648, cited in REFERENCES.md. Four values
had moved: the long-form vector format to `0xD4000000`, scalar memory to `0xF4000000`,
export to `0xF8000000`, and interpolation to `0xC8000000` - out of the slot the long form
now occupies, which is why every long-form arithmetic instruction had been decoding as an
interpolation.

### Surprises

- **The document is wrong about one field, and the solver caught it.** Its field table
  puts the LDS opcode at bits `[24:17]`. Under that, `ds_read_b32` decodes as 108 and
  `ds_write_b32` as 26; the document's *own* opcode table, forty pages earlier, says 54
  and 13 - which is what `[25:18]` gives. `orbistoun-gen encodings` had solved `[25:18]`
  from assembled bytes before the document was opened, and the disagreement is the only
  reason anyone checked the document against itself. The experiment corrected the
  reference, which is not the direction that pairing was built for.
- **This generation's long form can carry a literal; the previous one's could not.** All
  three of its sources sit in the *second* word, and the literal check only ever looked
  at the first, so every such instruction decoded four bytes short. `OperandField` now
  carries which word it lives in.
- **A skipped fixture left the previous generation's bytes on disk.** One source stopped
  assembling (the buffer formats were unified into one `BUF_FMT_*` enumeration), the
  generator reported the skip, and the stale `.txt` stayed - so the differential test
  compared this target's decoder against last target's output and reported the decoder as
  broken. The generator deletes the output now.
- **Six tests hard-coded an encoding**, and every one of them failed after the retarget
  while blaming the code under test rather than itself. A truncation test held a scalar
  load, a block-splitting test held one too, a pipeline test held an export, the
  self-consistency test held four words, and the whole agreement generator held every
  family and every opcode number. All of them ask the table now. **A test that writes
  down what it is testing stops testing it exactly when it matters.**
- **Operand code 125 names a register here and was a hole before.** It arrived as
  `<unmapped:125>` rather than as anything obviously wrong. Reads return zero and writes
  are discarded, and `v_div_scale_f32` uses it for a result it does not want.

### Outstanding

**`the_two_models_agree_on_generated_programs` faults the process** (`STATUS_ACCESS_VIOLATION`)
partway through its run on the device. Not the retarget, and not a bad program: it stops
after 43 programs from seed 1, and seeds 44-91 all pass when run *first*. So it is
cumulative state across roughly 86 dispatches rather than anything a particular shader
does - a resource the Vulkan runner is not releasing between dispatches is the obvious
suspect. Deliberately not chased by re-running it; the next step is to count live
allocations across dispatches rather than to bisect on a device.

