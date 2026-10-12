# D799 - A port state is its measured fields, the rest zero

**Status:** decided
**Date:** 2026-10-12

`sceAudioOut2PortGetState(port, state)`, for a port the process made, writes 0x40 bytes and answers
0. The first 0x18 are what a console wrote for its port in a fresh process (REQ-ao07,
`report-1791758178.txt` lines 77-86, the same both times): `81 00 02 00 ff ff 05 00` and sixteen
zero bytes - read as the port-state record a console's audio output reports, the output `0x0081`,
the channel count `2`, the volume `0xffff` and the reroute counter `5`, then the flags and reserved
words zero. The 0x28 bytes after them are written as zero.

Assumed (`known_by = assumed`): that those last 0x28 bytes carry nothing a caller reads. The console
wrote addresses of its own there (`0x8_000751d0`, `0x8_00692b50`, ...), which name memory in its
process and mean nothing in another; zero is what a caller that clears its record and reads only
the fields above would find unchanged.

**Why:** PPSA04263 asks for its first port's state once at start-up, and the call answered the
placeholder.

**Rejected:**
- Writing the console's addresses back verbatim: they point nowhere here, and a caller that followed
  one would fault on orbistoun's account.
- Writing only the first 0x18 bytes: the console wrote all 0x40, and a caller may lay its record out
  to that size.
