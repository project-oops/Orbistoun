# D134 - A named immediate the reference appends without a comma is still an operand


**Status:** assumed

The solver splits `ds_read_b32 v1, v2 offset:16` into three operands, not two. A named
immediate printed after the last operand with no comma before it - `offset:16`, and the
same shape elsewhere - becomes an operand in its own right.

Splitting on commas alone left `v2 offset:16` as one piece, which matches no register
pattern, so the opcode reported as unsolvable. That is the *good* failure. The bad one is
what happened before: every earlier local-share probe used the offset-free form, so the
field was in no sample at all and the solved layout simply had no slot for it. A
translator built on that reads the wrong word for every offset a compiler emits, and the
address register is still valid, so nothing complains.

This is the same trap the flat accesses set with their no-base form (D097's note): **a
probe set that omits a case teaches the solver the case does not exist**, and absence in
the samples becomes absence in the table. Every local-share probe now carries an explicit
offset, because the shortest sample decides how many operands an opcode has.

