# 875. The POSIX file calls fail the POSIX way

**2026-09-25**. Checking the obSCEne bus for Orbistoun requests that were resolved but never
actioned turned up REQ-20260914T1110Z-9b12. Its VirtualQuery half was taken up in worklog 868. Its
other half had sat unread since 2026-09-14:

- `open("/nonexistent")` answered `-1` with `errno = ENOENT`;
- `close(-1)` answered `-1` with `errno = EBADF`;
- so the POSIX-named exports use POSIX's convention, not the vendor `0x8002_0000 | errno` of their
  twins.

Orbistoun served the POSIX names with the twin's own function pointer (D349), so a failed
`close(-1)` answered `0x80020009`. Every POSIX knowledge entry still listed the convention as an
open question.

**Change.**

- `orbistoun_libc::posix_failure` turns a vendor error into `errno` set and `-1` returned, and passes
  anything else through.
- `open`, `close`, `read` and `write` are served by thin wrappers that call their twin and translate.
  `open` and `close` are measured. `read` and `write` follow as the same family, which is inferred,
  and their entries say so.

Test: `a_posix_file_call_fails_with_errno_not_a_vendor_code`. It checks that `close(-1)` answers
`0xFFFFFFFF` with `errno = 9`, and was watched failing with the translation bypassed, when the twin's
own `0x80020009` came back.

**The rest of the bus, checked.** Of Orbistoun's requests, only `d1c4` (pad field offsets, which
needs a controller in hand) is still open. The rest are resolved and cited in a worklog or knowledge
entry. `c9e2` (`sceAgcDriverSetTFRing`, `SetHsOffchipParam`) resolved as not exported on retail, so
their stubs stay as they are.
