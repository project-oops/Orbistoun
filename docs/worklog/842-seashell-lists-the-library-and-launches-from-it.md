# 842. SeaShell lists the library and launches from it

**2026-09-24**. A launcher now runs in orbistoun as it runs on the console.

- **Corpus:** `SCSH00001` is an archive source: the pinned SeaShell zip, unpacked into the library.
- **Directory reads:** `getdirentries` (554, FreeBSD 12 records) is implemented, plus
  `sceKernelGetdirentries` and `sceKernelGetdents`. The last two also serve 196 and 272 in the
  FreeBSD 11 layout. They read the same listing `opendir` gives.
- **Filesystem view:** `filesystem_view = "system"` is a per-title setting.
  - A compat record's `[settings]` is now built into the binary as the repository layer. The
    user's `overrides/<title>.toml` is merged over it (`Resolved::for_run`).
  - A title with this setting writes into one console-wide overlay (`console/`) and sees the
    library read-only at `/user/app`. SCSH00001 ships with it; every other title stays sandboxed.
  - Each title is mounted at `/user/app/<id>` by the id in its `param.json`, not by its folder
    name. Mounting the folder whole showed `PPSA02664-app0`, and SeaShell skipped every name that
    was not an id.
  - **Mount resolution now takes the most specific prefix.** It walked the table in key order, so
    `/user/app` answered for `/user/app/<id>/...` and a nested mount could never be reached.
- **Launching:** `sceSystemServiceLaunchApp` answers OK and hands the title id to the worker. The
  worker streams `Event::LaunchApp`. The GUI ends the run, starts that title from its library, and
  returns to the launcher when the title's run ends.
- **Pad:** `scePadReadState` and `scePadRead` now write the window's pad at the SDK's field
  positions, over the measured at-rest image (D713). With no pad sent, a read still answers the
  at-rest image unchanged.

- **One library:** the repo `titles/` folder is deleted. `bin/orbistoun`, the corpus commands'
  `--titles` default, the probe lookup (`PPSA99980`) and the import census now all read the
  shared library under the data directory.

**Result:** a headless SeaShell run discovers all 9 installed titles in `/user/app`, icons
included, through syscall 554.
