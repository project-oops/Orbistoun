# D668 - Twelve knowledge files nothing loaded

**Status:** decided
**Date:** 2026-09-10

## Written down, shipped, and never read

`crates/orbistoun-hle/data/knowledge/` holds twenty-four TOML files. Twelve of them were in
no build. `EMBEDDED` in `knowledge.rs` is a hand-kept list of `include_str!` calls, nothing
made it agree with the directory, and it had drifted by half.

`libSceNet.toml` was one of them. It has carried the measured would-block codes since D627 -
the entry that says a reimplementation guessing `0x8041_0000 | errno` would be wrong by
exactly `0x100` - and `Knowledge::builtin()` had never seen it. So did
`libSceAgcDriver`, `libSceCommonDialog`, `libSceCoredump`, `libSceErrorDialog`,
`libSceJson2`, `libSceKeyboard`, `libSceNetCtl`, `libSceSaveData_native`,
`libSceUserService`, `libSceVideoRecording` and `libkernel_unity`.

## Why this is worse than not writing them down

`orbistoun-cli learn` writes to these files. The knowledge-audit gate reads the *tree*. A
reader sees a populated directory. Every one of those says the behaviour is recorded - and
the emulator answered as though nothing were known.

That is principle 3 one level up: a tool reporting more than its measurement supports. The
accounting said "recorded" and the loader said nothing, and neither of them was in a position
to notice the disagreement.

## The fix is the guard, not the list

Registering twelve entries is thirty seconds of typing and would drift again the same way.
`include_str!` takes a literal path, so a compile-time directory walk needs a build script -
more machinery than this earns.

So: the entries are added, and a test reads the directory at test time from
`CARGO_MANIFEST_DIR` - the same base the `include_str!` paths are relative to, so the two
cannot be pointed at different places - and names any file the list does not carry. A second
test checks the other direction: that a file registered under a library name is not one whose
own `library =` line says something else, because `absorb` falls back on the registration name
and a mismatch would file a library's functions under another library's namespace.

**A guard is not finished until somebody has made it fail.** This one was written before the
entries and named all twelve.

## Found sideways

Nobody went looking. D667 needed a knowledge entry for `sceNetBind`, the entry was written
into a file that already existed, and the gate still said `sceNetSocket is implemented but
nothing is recorded about it`. The gate was right and the file was orphaned.
