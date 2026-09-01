# orbistoun-loader

Loads a guest module: parse, reserve, resolve, relocate, set up TLS, hand over the
entry point.

**Models:** all six steps. `survey` answers what a module needs without executing it;
`image::place` maps the segments, `relocate::apply` writes resolved imports into the
guest's slots, `tls::layout_of` reads the thread-local layout, `protect::apply` sets
final page permissions, and `process::build` assembles the process image.

**Deliberately fakes:** nothing.

**Design note.** The steps are ordered, and the third is where interception
happens: resolving each imported NID against the registry and writing the result
into the guest's relocation slots. There is no instrumentation pass.

`Survey::unresolved` is the number to drive down, and the honest headline for a
compatibility report - far more meaningful than a screenshot of a title that nearly
boots.

**Status:** done. Every commercial executable in the local corpus goes from bytes to an
entry point through this crate, and the guest then executes real code - which is the
moment the "interception is linking" design either worked or did not, and it worked.
`docs/ROADMAP.md` phases 3 and 4 are complete.
