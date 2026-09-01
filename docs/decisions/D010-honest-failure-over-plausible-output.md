# D010 - Honest failure over plausible output

**decided** · 2026-08-19

`Container::imports` errors rather than returning an empty list, because an empty
list reads as "this title needs nothing", which is never true. Generalises: never
invent a constant, error code, or arity to make something compile quietly. An
explicit "not handled yet" costs the same to write and is worth more.

