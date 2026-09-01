# D038 - Portable mode specification

**decided** · 2026-08-19

**Triggers**, OR'd: `ORBISTOUN_PORTABLE_MODE` env var · binary filename stem contains
`portable` (case-insensitive) · a `.portable` **directory** beside the binary.

**Root:** `./.portable/` beside the binary. Strict containment - nothing outside that
directory, **traces included**. An exception is what makes "does not touch outside
its own directory" false, and traces are a dev tool, so the size concern that
motivated a separate knob does not apply.

**Otherwise:** `directories::ProjectDirs` → `%APPDATA%\orbistoun\`. Portable is
opt-in; the plain binary is not portable.

**The sentinel is a directory, never a file.** Another project of mine hit `os error 183` on first
run because the sentinel and the data root are the same path - a `.portable` *file*
was written, then `create_dir_all` failed over it. The directory existing beside the
binary is itself the sentinel (`.exists()` passes for a directory), so a portable
install is sticky with no sidecar. GUI onboarding materialises it with an explanatory
`PORTABLE.txt` inside and heals a stale file from an older scheme.

Sentinel is checked **beside the binary**, not in the working directory: PWD-based
means `cd` into a folder containing `.portable` silently relocates the data root. The
per-project workflow that would give is better served by an explicit `--data-dir`
flag.

**Containment is a test, not a convention** - run in portable mode inside a temp
directory, exercise the writing paths, assert nothing was created outside the root.
That is what stops someone adding a `std::fs` call in six months.

Governs the GUI as well as the CLI, with a `[Portable]` title badge. Portable mode
governs where orbistoun *writes*, never where it looks for content: test material is
always an explicit argument. Lands as an `orbistoun-paths` crate, before the first
thing that writes.

