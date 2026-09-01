# 2026-08-27 - klogsrv prints its banner


Six functions after D343, a payload runs and speaks:

```
.----------------------------------------------------------------------.
| v0.9                                Copyright (C) 2025 John Tornblom |
'----------------------------------------------------------------------'
main.c:278:sysctl: error 0 (orbistoun has no message table)
```

The last line is **the guest naming its own next blocker, by file and line**, through this
project's own `strerror` text. klogsrv: 3 imports and 3 calls to **9 and 18**. ftpsrv: 1 to 8.

`signal`, `getopt`, `__error`, `strerror`, `fprintf`, `puts` (D348). Each one was chosen by
running the guest and reading what it asked for next - no guessing at any point.

### The half of the C library that is not a return value

Three of the six cannot be implemented as an answer. `getopt` reports through `optind`,
`errno` *is* `*__error()`, `strerror` hands back a buffer. D307 predicted this
(*"the first time the HLE layer would own state rather than functions"*) and D323 built the
storage; this session added the way back to it by name, so an implementation can leave
something where the guest will actually read it.

### Surprises

**`libc::` in a trace means declared, not implemented.** Chased a fault "inside
`libc::strerror`" for a while before checking the `nothing implements it` list, which had the
real answer. Declaring a function changes its label; binding it changes its behaviour, and
the two happen in different places (D082).

**Entering at `main` needed `argc`/`argv` to match.** Every `EntryArgument` variant answers
what a *process entry point* finds, and `main` is not one - it is a C function with a
documented signature. Handing it a process-argument block gives it a wild count, which
`getopt` then has to refuse rather than iterate.

**The tree refused all six until they were written down.**
`every_implemented_function_is_written_down` failed the moment `signal` was bound. Principle
1's accounting, working exactly as designed, catching the whole batch at once. Five recorded
`published`; `strerror` is `assumed` on purpose - its message is ours and says so rather
than imitating a table nobody here has seen.

### Not mine

`./orbistoun.sh prose` now fails on three line-continued literals in
`crates/orbistoun-service/src/symbols.rs`, a file this session never opened. That is the
fourth of the concurrent session's files to break a gate, after `orbistoun-submit`,
`orbistoun-gui` and `orbistoun-shell`. This session's crates: clippy clean, tests passing.


