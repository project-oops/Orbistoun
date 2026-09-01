# D023 - Release profile favours speed; debug info kept

**decided** · 2026-08-19

`opt-level = 3`, thin LTO, `codegen-units = 1`, and `debug = 1` with no stripping.
This is a hot-path emulator, so the size-optimised profile used for a container image
is the wrong trade. Line tables stay because guest crashes are unreadable without
them, and the size cost is paid once on disk rather than per frame.

