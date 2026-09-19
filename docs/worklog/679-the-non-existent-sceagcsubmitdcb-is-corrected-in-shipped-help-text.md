# 679. The non-existent `sceAgcSubmitDcb` is corrected in shipped help text

**2026-09-17** — inbox `-191c`. Four documents named `sceAgcSubmitDcb`, a symbol that is declared
nowhere: `grep -rn "sceAgcSubmitDcb" crates/` returns nothing, it is in no `guest_module!`, no
implementation list and no symbol table, so no trace can ever contain it. The real name is
`sceAgcDriverSubmitDcb` (`agc_driver.rs:28`, `libSceAgc.toml:473`, D120). One of the four is
`include_str!`d into the GUI as a help page, so the wrong name ships to a user.

## The damage, and the fix

The worst of the four (item 4) is advice: `USER_GUIDE.md:236` told a user to check the log for whether
"the title reached draw submissions (`sceAgcSubmitDcb`)" - to answer exactly the question this project
is organised around, look for a row that can never appear. Two others (`inspector.md:23`,
`USER_GUIDE.md:115`) are a sample inspector row showing it. The fourth (`graphics.md:7`, the GUI page
body) names it in prose. All four now read `sceAgcDriverSubmitDcb`, which a guest does import and call,
so the advice points at a row that can appear and the prose at a function that exists.

## The GUI page's subtitle disagreed with its own body

`crates/orbistoun-gui/src/app.rs:1476` titled the graphics help page "Vulkan 1.3 pipeline, RDNA2 shader
lowering, and detiling" - beside a body (`graphics.md`) that was rewritten to say presentation is not
implemented and the backend refuses every draw and present. The subtitle advertised a pipeline the
worker has no dependency on and a detiling feature that is one mode, one bit depth, one block, and not
wired to the run path. It now reads "PM4 command-stream decode and shader translation to SPIR-V;
presentation not implemented yet", which is what the page says.

## Scope note

The two sample inspector rows are a UI mockup inside a code fence, illustrating the inspector's columns
with example values; the fix is the name, per the request. Their illustrative "measured / 0x00000000"
is a column example, not a claim about a specific measurement, and is left as the mockup it is.

## Gate state

`grep -rn "sceAgcSubmitDcb" crates/ docs/` prints nothing (outside worklogs); `cargo check -p
orbistoun-gui` compiles the corrected subtitle; `./bin/orbistoun prose` exit 0; identity scan clean.
No commit.
