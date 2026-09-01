# 2026-08-19 (very late) - The first implemented function, and what it taught


D082-D083; 24 crates, 287 tests.

Wired declarations through to the guest - there had been **no path** from a declared
function to a running one, so implementing anything would have changed nothing silently.
Then implemented `sceKernelDirectMemoryQuery`, chosen because it is 99.9% of every call
four commercial executables make.

### Surprises

- **`ServiceConfig::default()` could not resolve a single import.** Empty hash suffix, so
  the worker's registry hashed to values nothing imports by. Everything built, everything
  ran, every lookup missed. A default that does not work is a trap, and it had been there
  since the suffix became available.
- **The guest ignores return codes entirely.** Ten candidate error values, spanning both
  signs, all behaved identically. An hour of reasoning would not have found that; a
  minute of sweeping did.
- **It reads the buffer instead**, and the terminal path was not writing it - so the
  guest re-read the previous answer forever. Clearing it changed the loop from
  `0 END END END` to `0 END 0 END`, which is the walk actually terminating.
- **A test caught me implementing an undeclared function.** `sceKernelDirectMemoryQuery`
  was implemented and never added to `guest_module!`, so it could never have been
  reached. The test existed precisely for that and fired on its first run.

### Outstanding

The guest walks the map, rejects one free 8 GiB region, and starts again. Sweeping single
fields has stopped paying - what is needed now is the structure's real layout, obtained
from a binary we wrote ourselves rather than inferred. **obSCEne stops being a roadmap
item here and becomes the tool the work requires.**

