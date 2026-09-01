# 2026-08-30 - The hardware had already answered; the results were on disk


Prompted to check whether the sysctl probe had run, it had - `data/hardware/ps5-imports.txt`,
a full suite on a PS5 at system software **12.400.009**, with the `135-sysctl` section in it. The
answers to a question this project had been refusing for good reason were sitting there.

`kern.osrelease` is **`0.0-prototype`**. Not a version - a development tag. `zftpd` reads it, cannot
parse a version out of it, and says detection failed. So the payload was correct against a value
nothing here knew, and D397's refusal to invent a plausible `9.00` was right: a made-up version
would have sent it down a different path and looked like it worked.

The console is **12.40**, stated outright in `kern.version`. The earlier 13.09 was the SDK version
from a different call; the platform keeps the two apart and now so does this. `machdep.tsc_freq`
came back `0x5f25_9b8e` - the same counter frequency the two clocks already reported, now by a
third independent route.

Implemented: `sysctlbyname` answers the measured strings and integers, each at the platform's own
width, pinned by a test. The 12.40 CEX profile is documented for the user to apply; the default
still refuses, because an unconfigured emulator does not get to claim it is a particular console.

The lesson to keep: **before building an instrument to ask, check whether the instrument already
ran.** The probe was written this session and the hardware run that used it happened in parallel;
the answer was a `grep` away from the whole time.

