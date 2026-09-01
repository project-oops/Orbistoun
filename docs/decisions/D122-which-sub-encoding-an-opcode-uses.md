# D122 - Which sub-encoding an opcode uses decides how its first word is read


**Status:** decided (2026-08-21) - and the list it describes is gone

The long-form vector ALU has two sub-encodings. One puts per-source absolute-value flags
in bits 8 to 14 of the first word; the other puts a **second, scalar destination** there -
a carry-out, or a flag saying an operand was pre-scaled. Nothing in the instruction says
which; only the opcode does.

`model::SCALAR_DESTINATION_OPCODES` lists them, and `Modifiers::read` takes whether this
is one. Without that, `vcc` as a carry destination - code 106, or 1101010 - presents as
"the second source is an absolute value", and an integer addition silently loses the sign
of an operand.

### Derived now, and the list is gone

The reasoning above for listing it was wrong, and the entry contains the refutation: *the
operand solver reports the fields*. It does - and the two sub-encodings differ in **exactly
those fields**. One has an operand in bits 8 to 14 of the first word; the other does not,
because in that one those bits are the modifier flags.

The answer was in the probe data the whole time, one table over. Checked before changing
anything: `v_add_co_u32` and `v_div_scale_f32` have a solved operand at word 0 shift 8;
`v_add_f32_e64` and `v_fma_f32` have nothing there.

So an opcode classifies itself, from evidence about that opcode, and the failure this entry
was written to warn about - an opcode added to `SUPPORTED` without being classified,
reading its modifiers from the wrong bits - **cannot happen**, because there is no longer a
second place to remember.

An opcode with no solved layout answers "no scalar destination" and that is safe: an
instruction whose operands are unknown is refused before translation begins, so this is
only ever asked about opcodes the solver has data for.

**A second copy went with it.** `touches_mask` also consulted the list, to decide whether a
shader needed the model with lane masks. It never needed to: a scalar destination that *is*
a mask arrives as a named operand and the operand check already catches it, and one that is
not is refused for a different reason. The list was doing the operand check's job with a
copy of its answer.

What remains is a test that the *data* still distinguishes them. If the solver ever stopped
recording that operand the classification would collapse to "none of them have one", and
every carry instruction would quietly start reading modifiers from its own destination -
which is the original fault arriving by a new route.

