# 2026-08-31 (later) - sceKernelGetSystemSwVersion made a profile setting (D421)


Followed the D420 value fix through to the right shape: the software version is now
`Machine::software_version`, presented from the machine profile exactly like `firmware`, instead of
a constant baked into the kernel. `SoftwareVersion { display, packed }` stores both the string and
the packed int because the rule relating them is undocumented (one sample, so deriving would be
inventing). Unset refuses, like an unset firmware. The decision is a pure `sw_version_write` so the
refusal, the null guard, the over-long-string truncation and the byte layout are all tested without
a guest buffer; the wrapper only does the copy.

The reference profile `ps5-cex-12.40` now carries `software-version = { display = "13.090.001",
packed = 0x13090001 }` alongside `firmware = 0x1240` - the two-numbers-one-console fact made data.
End-to-end verified: obSCEne's 130-layout dump under `--profile ps5-cex-12.40` emits 13.090.001 /
0x13090001 byte-for-byte, pass 0x28. Core/kernel/shell tests pass, clippy clean.

Gotcha worth keeping: a TOML sub-table under a dotted key needs the key quoted in the header -
`["ps5-cex-12.40".software-version]`, not `[ps5-cex-12.40.software-version]`, or the dots parse as
`ps5.cex.12.40.software-version` and the field silently deserialises to None. Caught by the profile
test (software_version was None); the honest default hid it until the test asserted the value.

