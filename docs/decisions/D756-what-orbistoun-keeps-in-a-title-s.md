# D756 - What orbistoun keeps in a title's directory is not in its /app0

**Status:** decided
**Date:** 2026-10-07

A title in the library runs with its directory as `/app0`, and the same directory holds what
orbistoun keeps for it: the link plan, the frames sheet, the kept translations and pipelines, the
save states and the guest's writable overlay. Those entries are hidden from every mount. A lookup
answers them absent and a listing leaves them out. Everything else in the directory, the title's
own files, answers as before. `Paths::title_state_entries` names the set once, and the worker hides
it when it establishes the sandbox.

**Why:** the console's `/app0` holds the package and nothing else. PPSA04263 walks `/app0` and
resolves every file it finds through the asynchronous file path, so it met `link-plan.json`,
`shader-translations.json` and a 47 MB `host-pipelines.bin` that no console shows it. A title that
reads what it finds in `/app0` - counts it, hashes it, opens it - takes a path it never takes on
hardware.

**Rejected:**
- Moving orbistoun's per-title files out of the title's directory. "Everything known about one
  title is one directory" is the library's layout (D724), and a library title would still be
  staged from there.
- Hiding by name everywhere. A title may ship a file called `frames.png`; only the paths orbistoun
  itself writes are hidden.
