# 2026-09-01 - stack read-ahead guard; obSCEne runs its whole suite under orbistoun


Next obSCEne wall (D444 oracle): `136-kernel/handoff` faulted at `image+0x4502ff` reading `0x600000801000`.
The handoff probe reads 20 words through `rdi` (the argument block orbistoun builds at the stack top); the
block sat flush against the reserved top with only `0x80` bytes of headroom, so a 20-word (`0xA0`) read ran
one word past the mapped stack. Fixed with a **readable guard page above the initial stack pointer**
(`GuestStack` `READAHEAD_GUARD`), mirroring the unmapped overflow guard below - a modest over-read of the
handoff now lands on mapped zeroes, as on hardware. Unit test reads both ends of the guard back as zero.

Result: **obSCEne now runs all 555 checks under orbistoun and reaches `OBS|end`** (was crashing ~check 40).
Tally `520/10/7/18` vs console `241/79/192/43`. The ten failures are enumerated in D445 as the next targets:
`virtual-query-stack`/`-text` (virtual_query blind to image/stack - one fix, two checks), `900-surface/control`
(resolver reports a non-existent symbol present), `110-modules` (module-list gap), `135-sysctl/osrelease` and
`137-kernelcall/system-version` (refused; measured on hardware, answerable from the report). mem tests pass
(23), clippy clean, fmt-clean.

