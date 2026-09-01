# D127 - Source modifiers are read from the raw words, and refused where unimplemented


**Status:** assumed

The vector ALU's long form carries per-source negate and absolute flags, a clamp flag and
an output multiplier. None of them is an operand, so the operand solver never sees them;
none is the opcode, so the encoding table never sees them either. They sit in bits both
layers correctly ignore, and every one changes the result.

`v_add_f32_e64 v0, v1, -v2` and `v_add_f32_e64 v0, v1, v2` differ by one bit. A
translator that read the operands and stopped would emit the second for both, and every
subtraction a compiler expressed as an addition of a negated operand would come out with
the wrong sign - in a shader that runs. That is the most likely way this crate could have
been quietly wrong at scale, which is why the bit positions were read off a reference
assembler rather than transcribed, and why there is a test whose only job is the order
the two flags apply in.

`neg` and `abs` are applied. Clamp and the output multiplier are **refused** - both change
the result, ignoring one produces a shader computing something close to right, and that
is harder to find than one that stops.

`Instruction` gained `second_word` so a translator can reach fields the decoder does not
model. The raw word rather than decoded flags, because which bits mean what is a property
of the sub-encoding and the decoder does not model sub-encodings.

