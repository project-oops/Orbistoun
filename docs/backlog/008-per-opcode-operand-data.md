# Per-opcode operand data


D096 established that a per-family operand layout only describes a family whose shape
is fixed - seven of seventeen. The other ten need operand data per *opcode*: how many
operands, and which fields carry them.

This is derivable rather than transcribable. The reference disassembler prints the
operands, so the generator can read their number and kind off the same fixtures it
already uses for mnemonics (D090), making every entry verified by construction and
growing only as the corpus grows.

It is the last thing between the decoder and a translator having everything it needs.

