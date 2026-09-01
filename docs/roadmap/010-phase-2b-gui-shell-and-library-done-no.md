# Phase 2b - GUI shell and library *(DONE; no output surface)*


`orbistoun-gui`. Large window, configurable library folder, per-title view. Menu
strip present with settings panes stubbed, populated as the subsystems behind them
land rather than built as dead UI.

**Why this early.** The library view needs only phase 1: listing titles and showing a
per-title import report requires the container parser and nothing else. The GUI is
useful long before anything boots, and it surfaces the honest metric rather than a
fake progress bar.

Per-title action is **Inspect**; **Launch** is present but disabled until phase 4 can
honour it.

**Observable result:** a window listing a real library, with a real per-title report
of what each would need.

**Done 2026-08-20.** `orbistoun-gui`, on egui (D161). Library, per-title inspection, and
**Launch is live** rather than disabled - phase 4 landed, so it can be honoured. The run
view shows the same progress verdict, stack-conformance line, ordered call tail and ranked
import list the CLI prints, from the same code (D160).

Deliberately absent: any output surface. The guest runs in a child process while the window
lives in the shim (D032), so presenting a frame needs a reparented window or shared images
- and there is no frame to present yet.

Writing it is what proved principle 13. Three things the CLI had absorbed came out within
the hour: the run comparison, the previous-trace load, and worker bootstrap. None looked
like logic in a shim until a second shim needed them.

