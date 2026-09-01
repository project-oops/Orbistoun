# D420 - sceKernelGetSystemSwVersion answers 13.09, which is not the 12.40 firmware


**measured** - 2026-08-31

Re-reading the *module* hardware reports (`console-klog`, `console-report-klog`, `ps5-full-run`)
after "is there really nothing left" turned up a value this project had wrong. The D416 SwVersion
work wrote `12.40.0.0` / `0x1240_0000` into `get_system_sw_version`, reasoning that 12.40 is "the
firmware this machine models". The module dumps refute it: `130-layout/system-software-version`
shows the call writing the string `13.090.001` and the packed integer `0x1309_0001`, identically
across all three runs.

The trap was assuming one console has one version number. It has at least two, and they disagree:

- **System software 12.40** - what obSCEne's `sysinfo` header carries (`firmware|known|12.40`), what
  the `kern.version` sysctl banner says (`releases/12.40 Nov 27 2025`), and what syscall 649
  ([`vendor_system_version`]) answers from `machine.firmware = 0x1240`. All correct, all left alone.
- **`sceKernelGetSystemSwVersion` = 13.09** - a *different* call reporting `0x1309_0001`. Not the
  firmware, and deliberately **not** sourced from `machine.firmware`: wiring it there would make it
  answer 12.40, which is exactly the reconciliation D416 made and hardware refutes.

So the fix is not "change the firmware to 13.09" - the firmware (12.40) was right. It is to correct
this one call's measured constant to `13.090.001` / `0x1309_0001`, and to write the reasoning down
loudly so the two numbers are not reconciled again. `machine.rs` already documented the distinction
("a conformance run read `13.090.001` from a *different* call ... not evidence about this one"); the
kernel had simply not followed it. Byte layout was already right (D416's refinement - string at 8,
int at 0x24, size word at 0 untouched); only the values were wrong.

Pinned with `the_software_version_is_thirteen_oh_nine_and_the_size_word_is_untouched`, which asserts
13.09 **and** that 12.40 is not what comes back - the regression that was made once. The wider
lesson is the CLAUDE.md one about reporting more than the measurement supports: D416 took "12.40"
from the project's own assumption, not from the dumped bytes, and the bytes said 13.09 the whole
time. Kernel tests pass (72), clippy clean.

