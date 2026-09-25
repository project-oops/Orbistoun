# 862. A signed-off capture takes Neverball in-game headless

**2026-09-25**. D721.

- **The capture.** The operator captured the route from Neverball's title menu into its first level
  with the toolbar's "capture input": 27 steps over 331 flips, taken from the menu. The capture
  began mid-run, so its flips count from there; replayed from entry it still lands on the menu,
  which waits for input.
- **The sign-off.** `orbistoun-gui --title NVRB00001 --playback <file>` played it back in the
  window. `--playback` is new: it arms a capture for the first launch, as choosing it from
  "playback input" before launching does. The operator watched it get in-game ("as expected").
- **Agreement.** The GUI playback and a headless replay both passed through the operator's exact
  menu states (`…1470 → …1cb0 → …1dd0 → …1bf0 → …1c50 → …1890 → …1950`).
- **Kept** as `corpus/input/NVRB00001-into-game.toml`, with its provenance in the header, and
  listed in `corpus/README.md`.

**In-game, headless** (`./bin/orbistoun run NVRB00001 -- --input
corpus/input/NVRB00001-into-game.toml --calls 0`): 27-28 flips a second, ~800 submissions and
~260k draws a second. The first level renders: course, ball, arrow, sky. The menus run at 15-16
flips a second because the menu draws the rotating level behind it.
