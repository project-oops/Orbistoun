# D011 - The container parser contains zero `unsafe`

**decided** · 2026-08-19

All structure reads go through `zerocopy`, which validates size and alignment first.
Parsing hostile bytes is the last place that should hold hand-rolled pointer casts.

