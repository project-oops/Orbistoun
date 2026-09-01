# D162 - A settings pane for a subsystem that does not exist is a dead control


**decided** · 2026-08-20

The shell was asked for application preferences covering resolution, window mode, and
video and input configuration. Those subsystems have **zero implementations** -
`orbistoun-audio`, `orbistoun-video`, `orbistoun-input` and `orbistoun-fs` declare their
interfaces and implement none of them.

A resolution dropdown over that is principle 3's failure mode wearing a widget. Somebody
picks 1080p, nothing changes, and there is no way to tell whether the setting is broken,
the emulator ignored it, or the title overrode it. **It is worse than no dropdown**,
because no dropdown at least says truthfully that nothing is configurable.

The roadmap called this before the window existed: *menu strip present with settings panes
stubbed, populated as the subsystems behind them land rather than built as dead UI.*

So the panes exist, and say what is missing and why. The structure is real - menu strip,
pane list, save - and it is populated the moment there is something behind it.

### What the preferences do carry, and why it matters more

Every control in the working panes changes something that takes effect on the next run,
and all of them were previously reachable only by hand-editing a TOML in the data
directory:

- **entry** - the convention and the first argument register. Today's alignment bug was
  found by flipping exactly this (D159).
- **threads** - the core count the guest is told about, and the affinity policy.
- **memory** - the direct-mapping switch.
- **general** - library folder and run limit.

These are not conveniences. They are the levers the bisection loop turns on, and the loop
is the only oracle most of this project has (principle 5). Turning them into a UI action is
the actual value of the window; a resolution dropdown would have been decoration over a
subsystem that cannot honour it.

### Per-title overrides are a text editor, deliberately

The override format carries compatibility entries with a **mandatory reason**, and keys
that name the behaviour rather than the title - `raytracing_enabled`, never `gta_rt_fix`.
That is what lets a second title needing the same thing add a line instead of a code path.

A form would have to either drop the reason field or invent a control for prose. Editing
the layer directly keeps the requirement visible until there is a design that respects it.

### Stop is real, or it is disabled

The toolbar's stop terminates the worker process, through a `Stopper` taken before the
handle is moved to the thread that blocks on it - the thread that owns the handle is
exactly the one that cannot act on a stop request.

Away from Windows there is no signal dependency in this build, so `Stopper::is_supported`
answers false and the control is **disabled with the reason on hover** rather than present
and silently doing nothing. Same rule as the panes: a control that lies is worse than a
control that is absent.

