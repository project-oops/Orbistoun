# D107 - The operand solver checks that a family agrees with itself


**Status:** assumed

After solving, `orbistoun-gen operands` groups fields by family and bit position and
warns when opcodes of one family read the same bits at different widths. A source field
is a property of the encoding, so at most one of those readings is right - the narrow one
solved without a sample high enough to need the top bit.

A warning rather than a failure: a field genuinely can differ between opcodes, and
refusing to generate would make an unprovable claim in the other direction. Naming it is
enough, because the cure is always the same.

Written because this failure has now occurred four times across three sittings - the wide
scalar loads, then `s_and_b64`, `s_or_b64` and `s_andn2_b64` each too narrow in a
*different* source slot. Every time it was found by a person putting the generated rows
side by side, which is a habit rather than a check, and the differing-slot case is
precisely what reading down a column misses.

Verified by removing the probes that fixed it and confirming the check reports both
disagreements, rather than by assuming a check that reports nothing is a check that
works.

