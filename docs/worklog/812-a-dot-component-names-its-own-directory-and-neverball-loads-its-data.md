# 812. A `.` path component names the directory it sits in, so Neverball finds its own data: 55 files open, the title music and level geometry load, and the event-pump spin gives way to real painting

**2026-09-24** — worklog 811 made the data wall visible: Neverball asks for `/app0/./data`, and
orbistoun answered ENOENT for a directory shipped in the title's own package.

## Why a `.` was refused

Neverball builds its data directory as `fs_base_dir() + "/" + CONFIG_DATA`, and upstream's
`CONFIG_DATA` is `./data`, so every path it opens reads `/app0/./data/...`. `mount::resolve_inner`
strips the mount prefix and hands the rest (`./data/...`) to `is_contained`, which accepts only
`Component::Normal` parts. `Path::components` drops a `.` in the middle of a path but keeps a
**leading** one as `Component::CurDir`, so the containment walk refused a path every POSIX lookup
accepts. `/app0` itself opened, which is why only the data failed.

## The fix

`without_current_dir` drops the `.` components (and the empty ones a doubled slash leaves) from the
part of the path under a mount, before the containment check. **Only `.`, never `..`**: a `.` stays
where it is, so dropping it cannot climb out of a mount or step through a symbolic link, and `..`
survives the collapse and is still refused by `is_contained` (D165). It runs after the whole-component
check, so `/app0.` is still a different name from `/app0`. The test
`a_current_directory_component_names_the_directory_it_is_in` pins all four: `/app0/./data/...`
resolves, `/app0/a/./b` resolves, `/app0/.` is the mount root, and `/app0/./../secret` and `/app0.`
are refused.

## What Neverball does now

```
orbistoun: the guest opened 55 paths:
  /app0/./data/back/space.png   /app0/./data/bgm/title.ogg   /app0/./data/geom/goal/goal.sol ...
standing 19999377 of 20000000 calls answered by an implementation (623 on stubs, 0%)
```

- **The data loads**: fonts, the theme, the title-screen music, ball and goal geometry and textures.
  Every data path still listed as *nothing here holds* was checked against the package on disk and is
  genuinely absent — the game probing `.jpg` before `.png`, and optional parts like
  `basic-ball-inner.sol` — so each ENOENT is the right answer.
- **The event-pump spin is gone**: calls on stubs fell from 978,723 to 623 (standing 96% → 100%).
  With nothing loaded the game had nothing to paint and spun on `sceSystemServiceReceiveEvent`; now
  it paints.
- **Painting is the new wall**: the game's own frame line reports `paint ~1010ms` per frame, and the
  budget run shows 475 frames against 7,242 before. Every frame now does real drawing work, and the
  hardware GL path is still failed on the unnamed submit hash `0x145f597e80e9876f` (worklog 809), so
  what those paints cost, and where, is the next question.
- **Writes to its own package still fail**: `/app0/.neverball`, `Scores`, `Replays`, `Screenshots` and
  `neverball.log`. The port measured `/app0` writable on hardware (its shim says so), and orbistoun
  keeps `/app0` read-only by design (D250) — a real disagreement to settle with evidence, not by
  flipping the rule.

The verdict reads `same` because it is computed from the import set and the call count, and a run
that ends on its call budget has the same count every time; it does not see the new code the guest
ran. The rerun's opened-paths list and the collapse in stub calls are that observation.

## Gate state

`crates/orbistoun-fs/src/mount.rs`: `without_current_dir`, its use in `resolve_inner`, and the test;
`orbistoun-fs` 120 tests pass. Generated docs regenerated after the run. `./bin/orbistoun check`
green, worklog index regenerated, identity scan clean. No commit.
