# D142 - One Vulkan loader, one instance, one device, for the life of the process


**Status:** decided (measured)

`the_two_models_agree_on_generated_programs` faulted the test process with an access
violation partway through its run on the device. It looked like a bad shader: it stopped
after 43 of 48 programs, reproducibly, twice.

It was neither the program nor the count.

- Seeds 44-91 all passed when run **first**, so seed 44's content was fine.
- A run with dispatch tracing completed all 96 dispatches and passed, so there was no
  threshold either. The fault was **intermittent** - about three runs in five - and
  everything about it moved when the timing changed.

**What it was.** `dispatch` built its entire Vulkan world per call and tore it down
again: the loader, an instance, and a device. A run that dispatched ninety-six times did
that ninety-six times. Vulkan is not built for it, and each layer of it failed
differently:

- `ash::Entry::load()` on every call loaded and unloaded the loader library **and every
  layer behind it** - overlays, capture tools, driver shims, of which a desktop machine
  has several. Layers keep process-wide and thread-local state that does not survive
  being unloaded and reloaded underneath a process still using it.
- `create_instance` / `destroy_instance` and `create_device` / `destroy_device` on every
  call did the same at the next level up.

**Caching the loader alone did not fix it** - two of three runs still faulted, which is
worth recording because it was the fix I was confident in. Only hoisting the instance and
device as well made it stop. Both changes are right independently; neither was sufficient
alone, and the measurement is the only reason that is known rather than assumed.

The loader, instance and device are now created once, behind a `OnceLock`, and never
destroyed - there is nowhere to destroy them from, and a process finished with Vulkan is
one that is exiting. A `Mutex` guards the session because queue submission and command
pools need external synchronisation and the harness runs tests on several threads.

**It was also nearly all of the runtime.** An instance and a device cost over a second to
create; dispatching a twenty-instruction shader does not. The Vulkan crate's device tests
went from 8.0s to 0.9s.

**What made this hard to see.** Every individual dispatch was correct - balanced maps, a
full teardown, `device_wait_idle` before releasing anything. The module even documents
its one known shortcut, that resources are abandoned on the error path, which drew
attention there instead. The bug was in the lines that read like setup rather than like
resources, and reviewing the release path - which is where the comments invite you to
look - would never have found it.

**A real emulator dispatches thousands of times a frame against one device.** The harness
should not have been shaped any other way, and the module's own note said as much: *if
this ever runs inside something long-lived, that has to change first.* A test binary
making ninety-six dispatches is something long-lived.

