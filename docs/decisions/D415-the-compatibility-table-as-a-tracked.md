# D415 - The compatibility table as a tracked markdown file, generated from the records


**assumed** - 2026-08-31

`compat list` ranks the records to the terminal; there was no artefact a person could read in the
repository, and the BACKLOG's "compatibility table" wanted one. `compat markdown` writes
`COMPATIBILITY.md` from `compat/*.toml` - the same ranking `list` prints (both rendered in
`orbistoun-overrides` so they cannot disagree, D184), as a document.

Two things it says that the terminal view does not. First, a **From** column: `run` for the honest
default-entry baseline, `experiment` for a run recorded with overrides, because those reach further
by construction and mixing them silently would be the D181 mistake. A record with only an
experiment slot is shown as such rather than dropped. Second, a **Screenshots** section: a guest
with graphical output embeds `compat/screenshots/<title>.png` and gets a 📷 in the table.

**Screenshots are a forward hook, and the file says so.** A real screenshot needs a captured guest
framebuffer, and the video subsystem does not surface one yet - the GUI's own capture is explicit
that it photographs the window, not a guest frame. So the section renders a note today and lights
up per guest the moment framebuffer capture lands; no tooling change needed then. It is a separate
command from `corpus run` deliberately: the run records, this renders, and a reader regenerates the
table without re-running anything.

Recorded `assumed`: a reporting-surface choice.

