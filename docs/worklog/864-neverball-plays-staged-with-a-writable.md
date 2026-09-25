# 864. Neverball plays, staged with a writable /app0

**2026-09-25**. Neverball now gets from its title menu into a level and plays it. Its states run
`st_title → st_name → st_set → st_start → st_level → st_play → st_play_set → st_play_loop →
st_fail`. The last is the ball falling off the level, because the script holds the stick back.
The run hits its 150 s limit with no fault.

**The wall.** At the level intro Neverball opens `/app0/.neverball/Replays/Last.nbr` for writing.
orbistoun's `/app0` was read-only for every title (D250), so the open failed and the game went to
`st_nodemo` ("a replay file could not be opened"). On the console the same port writes there: it is
staged under `/data/homebrew/<id>`, and that directory is its `/app0`.

**The change (D722).** Storage origin decides, never package metadata.

- **The library stages titles.** `titles/data/homebrew/<id>` is the staging tree
  (`orbistoun_paths::staged_under`). A corpus entry with `target = "staged"` syncs there. NVRB00001
  and NVPT00001 now do.
- **Origin.** A module whose directory lies directly under that tree is `Origin::Staged`. So is a
  loose build run with `run --staged` (`Request::Run::staged`). Anything else is `Origin::Image`.
  Tests: worker `a_title_is_staged_by_where_it_lies`.
- **`mount::stage_title`** layers the title at `/data/homebrew/<id>`, then puts one writable
  directory, `<overlay>/data/homebrew/<id>`, on top of it and of `/app0`, and allows writes under
  `/app0`.
- **Copy-up.** All write paths (descriptor/open `create`, `kernel_mkdir`, the POSIX calls) resolve
  through `resolve_for_write`, `resolve_for_create` or `resolve_for_removal`. None of them can reach
  a lower layer.
- **Discovery.** `discover_titles` and `bin/orbistoun` find staged titles, and prefer them over an
  image of the same name.

Tests: `only_a_staged_title_may_write_its_app0` and
`a_staged_write_to_a_shipped_file_never_reaches_the_library`. Both were watched failing:

- with the origin inverted;
- with the write resolving through `resolve`, as before, where the library file's bytes changed.

**Surprise: `/data` had the same bug.** A guest writing a file present in the base tree would have
rewritten the base tree, which is supposed to be rebuildable from its manifest. Copy-up fixes both.

**Not done.**

- Removing a name a lower layer also holds is refused, because that needs a whiteout.
- The other oops-apps titles are staged on the console too, but still sync as images. They stay
  that way until one of them needs its `/app0` writable.
