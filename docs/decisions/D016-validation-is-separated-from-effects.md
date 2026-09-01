# D016 - Validation is separated from effects

**decided** · 2026-08-19

`AddressSpace::validate` is a pure function over the ABI rules; `reserve` calls it
then acts. The rules are therefore fully testable without touching the host address
space. Prefer this shape - a pure decision function plus a thin effectful wrapper -
wherever it fits. In a codebase where most effects are hard to test, it is what
keeps coverage meaningful.

