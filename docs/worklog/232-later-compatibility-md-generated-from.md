# 2026-08-31 (later) - COMPATIBILITY.md, generated from the records (D415)


Added `compat markdown`: reads `compat/*.toml`, ranks them the way `compat list` does, and writes
`COMPATIBILITY.md` - the human-readable compatibility table the BACKLOG asked for. Rendering lives
in `orbistoun-overrides::render_markdown` beside `render_frontier` (D184), with two shape tests. A
**From** column separates the honest `run` baseline from `experiment` runs (D181); a title with
only an experiment slot is shown, marked, rather than dropped. Generated the first one: 33 titles,
obscene furthest (178 imports, ran to the limit), the 25 corpus payloads all at the `0x1`
default-entry baseline.

Screenshots are wired as a hook: the table embeds `compat/screenshots/<title>.png` when present and
marks the row with a 📷; with none present it renders a note explaining that a captured guest
framebuffer is what it needs, which the video subsystem does not surface yet (the GUI screenshot is
explicit that it captures the window, not a guest frame). So the section is ready and lights up the
moment framebuffer capture exists - no tooling change required. Kept as its own command, not folded
into `corpus run`, because the run records and this renders.

