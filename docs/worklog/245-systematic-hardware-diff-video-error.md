# 2026-09-01 - systematic hardware diff: video error family + single-output-ownership (D426)


With the full suite running, did a test-by-test and byte-by-byte diff of orbistoun vs the hardware
module report. Most divergences are non-actionable: orbistoun more correct (wcslen, is-stack, clean
handle writes), legitimately getting further where hw skipped on module-build symbol gaps, the
parallel kernelcall, or the all-mode census. Two were clean measured fidelity fixes, both done:
video-out invalid-handle now answers 0x8029000b (not the 0x7fff placeholder), and a second open of a
held output is refused 0x80290009 - so resolution-status skips as hardware does, and the whole video
section now matches test-for-test.

Byte-dump diff: 7 divergences, of which tsc_freq (8e vs dd) is boot-variance, SwVersion "untouched"
is leftover buffer content, and the rest are direct-memory region boundaries - orbistoun's model vs
this console's physical map, device-specific and low-value.

Out of clean small changes now. What remains: big subsystems (048-selfaudit needs getdents+SELF
enumeration; 165-gnm needs GPU command dispatch), judgment calls left deliberately (070-user
pre-init, 010-is-stack quirk - orbistoun's current behaviour defensible), an excluded-library
artifact (105-record, obSCEne drops libSceVideoRecording), and the device-specific memory map. None
is blocked on more hardware data; they are blocked on subsystem implementation.

