# D689 - The device enables what the modules declare

**Status:** decided
**Date:** 2026-09-14

The device requests `shaderInt16`, `shaderFloat16` and `fragmentStoresAndAtomics` where the
physical device offers them, and the capability report describes the enabled set, not the
physical device's. The draw path binds the guest-memory storage buffers at set 0 as the compute
path does.

**Why:** every emitted module declares 16-bit types, and a guest pixel shader writes guest
memory. A pipeline using features the device never enabled draws on one driver and is invalid on
the next, and nothing in the tests would notice. Reporting the enabled set keeps downstream code
from building on a feature nobody asked for.

**Rejected:**
- Refusing modules that declare storage buffers: refuses every module the translator emits.
- A fragment variant with no storage buffers: silently changes what the shader does.
- Relying on the driver to tolerate it: an invalid pipeline that passes on one machine.
