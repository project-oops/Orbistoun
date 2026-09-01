# The crash was the harness rebuilding Vulkan ninety-six times


The access violation left open at the end of the retarget. `dispatch` built its whole
Vulkan world per call - loader, instance, device - and tore it down again, so a run that
dispatched ninety-six times did that ninety-six times. All three are now created once and
never destroyed (D142).

Four consecutive runs of the test that was faulting three times in five now pass, and it
takes **4 seconds instead of 120**.

### Surprises

- **Every wrong hypothesis died to one cheap experiment**, which is the only reason this
  took an evening. Seed 44's content: run seeds 44-91 first, they pass. A dispatch-count
  threshold: count them, and the run that counted completed all 96 and passed. That last
  result is what reframed it - a run that passes *while being observed* is not a data bug.
- **The fix I was confident in was not sufficient.** Caching the loader is a real bug fix
  and it is genuinely wrong to load and unload `vulkan-1.dll` per call - but two of three
  runs still faulted afterwards. Only hoisting the instance and device as well stopped it.
  Worth writing down because I had already recorded the loader as *the* cause before
  measuring, and had to go back and correct the decision entry.
- **The module's own documentation pointed at the wrong place, honestly.** It says
  resources are abandoned on the error path and that this would have to change if
  anything long-lived used it - true, prominent, and not the bug. The bug was in the
  lines that read like setup rather than like resources.
- **Every individual dispatch was correct.** Balanced maps, `device_wait_idle` before
  teardown, every handle destroyed exactly once. Reviewing the release path, which is
  where the comments invite you to look, would never have found it.


