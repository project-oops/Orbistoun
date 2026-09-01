# D123 - An opcode with no shape row is an error, not a skip


**Status:** assumed

`Builder::check` verifies every emitted opcode has a row in the shape table, before it
checks anything about identifiers.

Skipping unknown opcodes was the original design and the reasoning was sound in
isolation: guessing an unknown instruction's shape would mean reading a literal as an
identifier and complaining about a module that is fine. What that reasoning missed is
that a skipped instruction's **result** is never recorded either - so the next instruction
to use it is reported as referring to nothing.

That is a false failure naming the wrong instruction, which is worse than either guessing
or silence, and it happened the first time an opcode was added without its row. The check
now says so directly.

The general shape is worth keeping: **a check that degrades on unknown input has to be
asked what it does with the parts that depend on the unknown**, not only what it does
with the unknown itself.

