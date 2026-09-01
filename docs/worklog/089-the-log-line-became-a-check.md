# The log line became a check


D128 widened a too-narrow field from the rest of its family and printed what it had done.
Two very different situations produced identical output - a probe that could have been
written and was not, versus an operand no instruction can push that far - and the only
thing separating them was a line nobody reads.

It is answerable by asking the assembler: put a value in that operand that would need the
wider field, and read the field back out. Both current adoptions check out as genuinely
unreachable, so the original claim stands, now with evidence.

248 tests green.

### Surprises

- **My first version of the check was wrong in the dangerous direction.** It asked "did it
  assemble" and answered *avoidable* for both - because `s[100:101]` assembles fine, and
  its code is 100, which fits in seven bits and needs nothing wider. It would have sent
  someone to widen a probe for a field no instruction can reach: exactly the advice the
  check exists to prevent. Verifying my own check against the assembler is what caught it,
  and I nearly trusted it because it agreed with what I already suspected.
- **A check that can only stay silent is worth nothing**, so it is tested in both
  directions - asked about an ordinary source, which takes a vector register whose code
  needs the ninth bit, it fires.

