# D161 - The GUI is egui on eframe

**Status:** decided
**Date:** 2026-09-26

`orbistoun-gui` is an immediate-mode application on egui, through `eframe` with the wgpu
backend. The binary checks for worker mode before any window exists.

**Why:** its panels - ranked imports, a call tail, registers, a verdict - are replaced wholesale
when a run finishes, which is what immediate mode draws well. Presenting a guest frame later is
clearer through wgpu than through a webview. The same executable is re-invoked as the worker, so
reaching window code there would open a window per launch.

**Rejected:**
- A webview shell: guest frames would be blitted into it.
- A retained-mode toolkit: effort spent syncing state that is thrown away.
