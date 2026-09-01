# D121 - Signed overflow is computed in bits, not compared


**Status:** assumed

`s_add_i32` sets the condition code on signed overflow: the operands agreed in sign and
the result does not. Expressed as exclusive-ors and a sign-bit test rather than by
comparing values, because there is no core opcode for "did that overflow" and the bit
form is exact for every input including the extremes, where a comparison-based test needs
care about the wrapping it is trying to detect.

Subtraction uses the same test with the right-hand side's sign flipped, which is the same
statement about a negated operand.

