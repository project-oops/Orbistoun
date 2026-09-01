# D140 - A zero-operand entry may be solved from one sample


**Status:** assumed

The two-sample rule exists so a *field* is not inferred from one observation. An entry
with no fields has nothing to infer and nothing a second sample could corroborate - what
it carries is the opcode's name, which one observation establishes as well as ten.

`s_endpgm` is the case: no operands, and it assembles identically every time. Demanding
two samples would be satisfied by duplicating a line in the probe file, which meets the
letter of the rule and none of its purpose.

