# D421 - The software version is a profile setting, like the firmware, not a kernel constant


**assumed** - 2026-08-31

D420 corrected the value but left it hardcoded in `get_system_sw_version`, which is the wrong
*shape*: firmware version (12.40) is a `Machine` field set by a profile (principle 5, "nothing about
a specific firmware version is compiled in"), and the software version this call answers should be
too. It is now `Machine::software_version`, an `Option<SoftwareVersion>` presented from the same
profile the firmware comes from, read by the kernel exactly as `firmware` and `kernel_release` are.

Three choices worth recording:

- **Both representations are stored, not one derived from the other.** The struct writes a display
  string (`13.090.001`) at offset 8 and a packed integer (`0x1309_0001`) at 0x24, and the encoding
  relating them is not documented - stripping the dots and reading as hex happens to work for this
  one sample and demonstrably fails for others, so deriving would be inventing an arity (principle
  3). `SoftwareVersion { display, packed }` states both, and a profile that gives one without the
  other is refused by deserialisation, not half-answered.

- **Unset refuses**, the same honest default `firmware` (0) and `kernel_release` ("") keep: a
  machine with no software version returns `NO_ENTRY` rather than a made-up one a guest would read
  back and branch on. This is a behaviour change - the default machine now refuses the call where
  the hardcode used to answer - and it is the correct one: measurements were always taken under the
  reference profile, which carries the value.

- **The decision function is pure.** `sw_version_write(Option<&SoftwareVersion>, out)` returns the
  refusal code or the `(dest, bytes)` to write, so the refusal, the null-destination guard, the
  truncation of an over-long string, and the byte layout are all pinned without a guest buffer
  (principle 8). The effectful wrapper only does the identity-mapped copy.

Recorded `assumed` on the field's *shape* (the `SoftwareVersion` struct, the refuse-when-unset
choice) - the *value* is `measured` under D420. End-to-end: obSCEne's `130-layout` dump under
`--profile ps5-cex-12.40` now emits `13.090.001` / `0x1309_0001` byte-for-byte, `pass 0x28`. Core,
kernel and shell tests pass; clippy clean. The TOML sub-table header had to quote the dotted profile
name (`["ps5-cex-12.40".software-version]`) or the dots parse as key separators.

