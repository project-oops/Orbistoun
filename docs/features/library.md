# The library

The library is the set of titles orbistoun has found in the library folder. The list view
shows it down the left of the window with the selected title's details beside it; the shell
view shows it as tiles. On the command line the same information comes from
`orbistoun-cli inspect`, `imports` and `compat list`.

## Where titles come from

Orbistoun ships no titles. It scans one folder, set under preferences - general as the
library folder. The default is `titles`, relative to the data root, which is the
collection's shared `titles/` directory (see [where it writes](paths.md)); an absolute path
is used as given (D038).

A **title** is a directory holding an `eboot.bin`. The scan also reads
`sce_sys/param.json` for the title's name, ID, version and required system version, and
`sce_sys/icon0.png` for its icon; a title without them is named after its folder. Titles
staged under the folder's `data/homebrew/<id>` tree are found too, and a staged copy takes
precedence over another of the same name, because its storage is the one a title can write.

The scan reads each title far enough to describe it and never executes anything, so it is
safe to point at a folder of unknown files. It runs when the window opens and again on file -
rescan library, the toolbar's refresh, or settings - reload settings file.

## The list

Each row shows:

| Line | Shows |
|---|---|
| first | the title's name |
| second | its title ID, and `fw <version>` when it states the system version it requires |
| third | the last run: how many distinct imports it called, then where it ended - `<region>+<offset>` for a fault, or "ran to the limit". "never run" when it has no trace. |

Click a row to select it; double-click to run it. When the scan fails the panel shows why, and
when the folder is empty it shows which folder was scanned. Under either message is the
settings file that chose the folder, or "no such file" when every setting is a default.

The bottom of the panel shows which build this is: a commit, or when the binary was compiled
when there is none. Hover it for the full form, to paste into a report.

## The detail panel

The right side of the list view describes the selected title.

| Part | Shows |
|---|---|
| header | icon, name, and a line with title ID, version, required system version, the toolchain it was built with when it says, and the folder name |
| run result | after a run, what it produced; see [running a title](running.md#the-run-result) |
| imports | `N of M named`: how many of the title's imports the symbol database can name; see [names and hashes](naming.md) |
| container | the container's structure as `orbistoun-cli inspect` reports it, or why it could not be read |

The import list is knowable before anything runs because interception is linking: the guest
imports by hash and the loader resolves the whole table before the first guest instruction,
so the complete set of demands a title makes is in hand in advance.

## Title overrides

settings - title overrides..., or configure on the toolbar, opens the selected title's
override file as text. It is saved to `overrides/<title>.toml` under the data root. Settings in
it are merged per key over the shipped defaults, so a key left out keeps its value. A
compatibility entry names the behaviour it changes, never the title, and carries a mandatory
reason. A file that does not exist opens as a commented template.

## The shell view

The shell view presents the same library as rows of tiles, moved through with the pad's
directions and chosen with its south button, or with the pointer.

| Row | Holds |
|---|---|
| user | the signed-in user; choosing it opens the settings |
| titles | one tile per title, with its icon; choosing one runs it. The highlight starts here. |
| settings | console and controllers (the preferences window), rescan the library, developer list view |
| power | quit the title (while one runs), close orbistoun |

The footer shows the build and the renderer the window is drawing with. When the library
cannot be read, or is empty, the titles row says so and offers "look again".

## Reach

The compatibility record of each title, generated into [COMPATIBILITY.md](../../COMPATIBILITY.md),
ranks titles by **reach**: `rejected`, `parsed`, `linked`, `entered`, `exited`, `flipped`,
`presented`. `flipped` means the guest got a frame to the output layer, a place reached rather
than a picture judged; `presented` means a buffer whose pixels differ from what it held before
the guest ran. `orbistoun-cli compat list` prints the same ranking.

## On the command line

```bash
orbistoun-cli inspect <title>/eboot.bin          # the container's structure, without executing it
orbistoun-cli imports <title>/eboot.bin          # what it imports
orbistoun-cli imports <title>/eboot.bin --own    # the modules it ships that answer its own imports
orbistoun-cli exports <module>                   # what a module provides
orbistoun-cli compat list                        # every recorded title, furthest first
```
