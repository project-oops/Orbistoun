# 2026-09-01 (/loop) - error-code sweep completed: 5/6 rejects-* checks match hardware (D438 cont.)


Fixed the failing event-flag integration test (D438's own change - it pinned the old 0x7fff0003; updated
to NO_SUCH=0x80020003, which still differs from a miss/BUSY so the test's point holds; kernel tests green
74+33+33). The fs suite has pre-existing parallel-execution flakiness (sendfile passes in isolation) and a
hanging socket test - not from these changes. Implemented sceAudioOutClose to return the audio subsystem
NO_SUCH (0x80260003, base 0x8026_0000 via the existing vendor_in), matching hardware; scePadClose already
returned 0x80920003. Verified via the probe. So 015/020/040/090/100 now match hardware; 060-module/dlsym
still needs handle validation (design change, deferred).

