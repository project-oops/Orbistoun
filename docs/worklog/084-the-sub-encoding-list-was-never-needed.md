# The sub-encoding list was never needed


D122's own entry carried its refutation. It said the classification had to be listed
because *the operand solver reports the fields without naming which sub-encoding produced
them* - and the two sub-encodings differ in exactly those fields. One has an operand in
bits 8-14 of the first word; the other has modifier flags there.

Checked before changing anything: the solved layouts already distinguish them. The list is
gone, an opcode classifies itself, and the failure the entry was written to warn about
cannot happen any more because there is no second place to keep in step.

A second copy of the same list went with it, in `touches_mask`, which was using it to
answer a question the operand check already answered.

245 tests green across the shader-side crates, clippy clean.

### Surprises

- **The entry described the fix and then argued against it.** "The operand solver reports
  the fields" was written as the reason listing was necessary. It was the reason it was
  not.

