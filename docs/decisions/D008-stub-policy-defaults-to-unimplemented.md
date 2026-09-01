# D008 - Stub policy defaults to `Unimplemented`, never `Ok`

**decided** · 2026-08-19

A stub that reports success is indistinguishable from working code until forty
thousand frames later. Loud by default; relax individual functions deliberately.

Policy is a runtime TOML file keyed by human-readable symbol name, because editing
it and relaunching **is** the bisection workflow - the only oracle most functions
have (see [TESTING.md](../TESTING.md)).

