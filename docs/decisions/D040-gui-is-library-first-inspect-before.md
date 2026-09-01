# D040 - GUI is library-first; Inspect before Launch

**decided** · 2026-08-19

Large window defaulting to a configured library folder; selecting a title acts on it;
menu strip with graphics/sound/input configuration stored in the user profile.

**The library view needs only phase 1.** Listing titles and showing a per-title
import report - "needs 412 functions, we have 37" - requires the container parser and
nothing else: no memory manager, no threads, no renderer. So the GUI is useful long
before anything boots, and it surfaces the honest metric rather than a fake progress
bar.

GUI is therefore two pieces of work, not one: **shell plus library** right after
phase 1, and **output surface** at phase 6. Settings menus exist structurally from
the first, with panes populated as the subsystems behind them land rather than built
as dead UI.

Per-title action is **Inspect** now; **Launch** appears once phase 4 can honour it.

