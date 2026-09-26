# D099 - Translated shaders are executed on a device

**Status:** decided
**Date:** 2026-09-26

Translated shaders are dispatched on a real Vulkan device and their output read back. Vulkan is
loaded at run time, and one loader, instance and device serve the whole process. A missing
device is reported as a skip, loudly, by the check gate.

**Why:** a validator says a module is well-formed, not that it computes the right thing. Linking
Vulkan would make the build need an SDK. Loading and tearing down the loader and device per
dispatch faults intermittently and costs a second a time.

**Rejected:**
- Validation only: well-formed and wrong passes.
- Linking the Vulkan loader: the build depends on an installed SDK.
- A device per dispatch: intermittent faults and most of the runtime.
