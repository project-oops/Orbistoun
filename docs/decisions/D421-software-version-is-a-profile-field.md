# D421 - The software version is a profile field, not a kernel constant

**Status:** assumed
**Date:** 2026-08-31

`sceKernelGetSystemSwVersion` presents `Machine::software_version`, read from the active
profile like the firmware version, instead of a value compiled into the kernel.

**Why:** A profile already supplies the machine's firmware and kernel-release strings; the
software version is the same kind of fact and must vary the same way. Storing both the display
string and the packed integer, rather than deriving one from the other, avoids inventing an
encoding no lawful source documents. A machine with no software version set refuses the call
rather than answering a made-up value.

**Rejected:** deriving the packed integer from the display string by stripping dots and reading
hex - happens to work for one sample and is not a documented rule.
