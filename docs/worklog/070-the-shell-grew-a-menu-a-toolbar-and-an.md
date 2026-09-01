# The shell grew a menu, a toolbar, and an argument about dead controls


Asked for the usual shape: selectable library, toolbar whose buttons enable on selection,
double-click to launch, menu bar for application preferences, toolbar button for per-title
settings. All of it built, and all of it against machinery that already existed.

- **toolbar** - start, stop, configure. Disabled rather than hidden when they do not
  apply, each with the reason on hover.
- **double-click launches**, and selects at the same time, so the panel behind is never
  showing a different title than the one that started.
- **menu strip** - file, settings, help.
- **preferences** - pane list, save, "settings apply to the next run" stated plainly.
- **per-title overrides** - opens the user layer, with a commented template when the file
  does not exist yet.

**Stop is real.** It terminates the worker through a `Stopper` taken *before* the handle
moves to the thread that blocks on it - the thread that owns the handle is exactly the one
that cannot act on a stop request. Where it cannot work it is disabled with the reason.

### The one thing not built as asked

Resolution, window mode, video and input config. Those subsystems have zero
implementations - they declare interfaces and implement none of them - so those controls
would do nothing while looking like they worked. That is principle 3 in a dropdown, and
the roadmap had already ruled on it: *panes stubbed, populated as the subsystems land
rather than built as dead UI* (D162).

The panes exist and say what is missing. The structure is there for the day it is real.

What the preferences carry instead turned out to be the better half anyway: the entry
convention, the thread policy, the direct-memory switch - the levers the bisection loop
turns on, every one of which previously meant hand-editing a TOML. Today's alignment bug
was found by flipping one of them.

Verified: seven crates clean on `-D warnings`, no failing suites, window opens and stays
up, CLI byte-identical afterwards (46 imports, 799 conforming calls).

