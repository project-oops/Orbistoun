# D235 - The initialiser tags are parsed, and the answer was that there are none


**decided** · 2026-08-25 · hypothesis measured before it was built on

Having eliminated every call in PPSA02664 as the source of the missing region base
(D229, D230, D234), what was left was "not a call at all". The dynamic-table parser handles
`NEEDED`, `STRTAB`, `SYMTAB`, `RELA`, `JMPREL` and the vendor import tags, and falls through
on everything else - including `DT_INIT`, `DT_INIT_ARRAY` and `DT_PREINIT_ARRAY`.

That fit the evidence uncomfortably well. A C++ namespace-scope object with a constructor
gets an `init_array` entry; a function-local static does not, because it initialises on
first use behind a guard variable. So a guest whose `init_array` never ran would show guard
traffic in the trace - and PPSA02664 makes fifteen `__cxa_guard_acquire` calls - while every
global object stayed zero. A region base held in a global would read as `0`, and
`0 + 0x100000 - 0x20` is the faulting address exactly.

**It is wrong.** The tags are parsed now, and `init_array` is absent from all three titles
at a wall: `init_arraysz = 0`, no entries. There was nothing to run.

Recorded because the hypothesis was good and the discipline is the point: the next step
would have been to build an initialiser executor, and it would have executed an empty list,
run identically, and been indistinguishable from a correct implementation. Measuring first
cost one temporary print.

The parsing stays. The absence is now a measured fact with a test behind it rather than a
gap nobody had looked at, and the loader can say so instead of not knowing.

### Two things left over

`DT_INIT` reads **`0x10` on all three titles** - identical across three unrelated games, and
not a plausible code address when the entry point is `0x70`. Either tag 12 does not mean
`DT_INIT` in this container, or it is not an address. Unresolved, and not guessed at.

PPSA28061 carries a `preinit_array` pointer with **size zero**, which is an array of no
functions. Also unexplained.

### And a third, which is why the second one mattered

The *save* button in the preferences window was not reachable. Laid out in reading order -
panes, separator, actions - the `ui.separator()` between the pane list and the pane is a
vertical rule inside a horizontal layout, so it grows to fill the available height; and in
a window that sizes itself to its content, "available" is however much screen there is. The
action row was pushed past the bottom edge, and the only way to press *save* was to tab to
it blind.

So the one control that could have fixed the library from inside the window was the one
control the window had hidden. The actions are now a bottom panel placed *before* the
panes, the content scrolls, and the window opens at a fixed size.

### A note on where the file was, which was none of the above

The settings file this chased did exist - twice, byte-identical, under two paths - and the
window was right that it could not see either. The tooling writing it was running inside a
packaged application with AppData redirection, so `%APPDATA%\orbistoun\data\config.toml`
resolved to a private per-package copy and was reported back under the original name.
Every check agreed with every other check and all of them were answering a different
question than the window was.

Nothing in this repository caused it and nothing here can prevent it. It is recorded
because the diagnostics added above are what eventually made it visible: the window naming
the file it read, and saying plainly that it was not there, is what turned "the scanner is
broken" into "these two processes disagree about a path" - which is a question with an
answer.
