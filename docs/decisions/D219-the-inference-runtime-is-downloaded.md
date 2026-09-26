# D219 - The inference runtime is downloaded

**Status:** decided
**Date:** 2026-08-24

Accelerated inference uses a prebuilt `llama-server` with its Vulkan backend, downloaded under the
supplied root like a model, started as a child process and addressed over the wire format the
crate already speaks. The device is named from the runtime's own enumeration. A proposer varies
the seed between rounds.

**Why:** a build-time accelerator feature needs a vendor toolkit and produces a binary that loads
nowhere else. Any machine that runs this project has a Vulkan driver, and the archive carries
processor backends for machines without a device. A named device turns an unusable one into a
refusal rather than a silent processor fallback. A proposer's oracle is a hash, so it needs
variety, not determinism.

**Rejected:**
- A CUDA build feature: a toolkit at build time, one vendor, an unportable binary.
- Parsing the runtime's log for the device: a debug stream, not an interface.
- Greedy decoding for proposals: repeats within and between rounds.
