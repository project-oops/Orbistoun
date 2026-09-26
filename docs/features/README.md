# Orbistoun features

One page per feature of the two programs: `orbistoun-gui`, the desktop window, and
`orbistoun-cli`, the command line. Both are shims over the same crates, so every page covers
both. The window shows these pages under help - documentation.

| Page | In the window | On the command line |
|---|---|---|
| [User guide](user-guide.md) | starting the window, its views, menus and toolbar; troubleshooting | `orbistoun-cli --help`, the quickstart |
| [The library](library.md) | the library list, the shell view, the detail panel | `orbistoun-cli inspect`, `orbistoun-cli imports` |
| [Running a title](running.md) | start, stop, the run result, preferences, the machine profile | `orbistoun-cli run`, `report`, `verify` |
| [Inspecting a run](inspector.md) | the run result's call lists and events | `OOPS_LOG=trace`, `orbistoun-cli knows`, `worklist` |
| [Memory](memory.md) | the memory preferences pane | `orbistoun-cli load`, the memory diagnostics |
| [Graphics](graphics.md) | the running title's picture and the performance overlay | `ORBISTOUN_PROFILE`, `ORBISTOUN_TRACE_SUBMITS` |
| [Controllers](controllers.md) | the controllers pane, input capture and playback | `run --input`, pad scripts |
| [Names and hashes](naming.md) | the import count in the detail panel | `orbistoun-cli verify`, `names`, `harvest`, `nid` |
| [Where it writes](paths.md) | the settings file line under the library | `orbistoun-cli paths` |
