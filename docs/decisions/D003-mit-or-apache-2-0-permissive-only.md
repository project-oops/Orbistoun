# D003 - MIT OR Apache-2.0; permissive-only dependency policy

**decided** · 2026-08-19

Emulator projects often choose GPL specifically to prevent closed forks. Considered
and declined - preventing closed forks is not a goal here. `deny.toml`'s allow-list
is therefore permissive-only *deliberately*, and a GPL dependency would relicense
the binary. This is settled, not a default awaiting review.

**Amended 2026-08-19: MPL-2.0 is allowed.** `cargo-deny` rejected `option-ext`
(reached via `directories` -> `dirs-sys`) on the original permissive-only list. The
concern this decision records is *binary relicensing*, and MPL-2.0 is **file-level**
copyleft - it obliges publishing modifications to MPL-covered files and does not reach
our code. We consume the crate unmodified. GPL and AGPL remain excluded; the
distinction between file-level and binary-level copyleft is the point.

Considered and rejected: dropping `directories` and resolving OS data directories by
hand. Roughly thirty lines, but it trades a well-maintained crate that handles
platform quirks correctly for hand-rolled XDG and macOS path logic - a worse end state
under D028, and for a licence concern that does not actually apply.

