# D021 - The ABI spike happens out of order

**decided** · 2026-08-19

Roadmap phase 0b, independent of everything. As the chain is ordered, guest code
first executes at phase 4, by which point the parser, symbol loading, and address
space are built on an unvalidated assumption about calling convention, stack
alignment, and TLS layout. A day of throwaway hand-assembled code answers that up
front.

