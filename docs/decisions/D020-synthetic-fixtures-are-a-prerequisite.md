# D020 - Synthetic fixtures are a prerequisite, not a nice-to-have

**decided** · 2026-08-19

Roadmap phase 0. The parser cannot be developed without something to parse, and per
D014 no real container can ever live here. Fixtures are also the **only form of
ground truth this repo can hold** - every other layer is inference against a black
box - so they are worth building properly.

