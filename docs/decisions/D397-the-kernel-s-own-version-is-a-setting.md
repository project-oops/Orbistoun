# D397 - The kernel's own version is a setting with no default


**assumed** - 2026-08-30

> **Resolved 2026-08-30 (D405):** hardware answered `kern.osrelease` = `0.0-prototype` - a
> development tag, not a version, which is exactly why `zftpd` could not parse a firmware out of
> it. The refusal here was correct; the value is now measured and configurable.

`sysctlbyname` was unimplemented, so it answered a placeholder - and for a call returning a
length, a placeholder is data. `zftpd` asks it exactly once, for `kern.osrelease`, and reports
`Firmware detection failed` before turning a feature off.

It is a named question with a recorded answer now, in the same shape as the numeric `sysctl`:
what is known is answered, what is not is refused **and reported once**, so the names a guest
wanted are the work list rather than a silence.

### And the answer is deliberately absent

`kern.osrelease` comes from the machine's own configuration and is **empty by default**.
Nothing in this repository knows what a console's kernel calls itself - the FreeBSD checkout is
not that kernel, and the mined name lists are names rather than values.

A guest **branches on this**. Answering something plausible would send it down a path chosen by
a number nobody has measured, and the run would look like it worked - which is the failure this
project spends most of its time avoiding. Empty refuses the question, which `zftpd` handles by
saying so and carrying on.

### What the measurement showed, and where it stops

With a value set the refusal disappears - the call answers and the guest reads the string. It
still reports detection failed, so it **parses** the string and expects a shape `9.0` is not.

That is the honest stopping point: the mechanism is complete and correct, and the value is a
measurement nobody here has. One string from a real console finishes it, and until then the
setting is the place it goes rather than a constant in a function.

### Where it lives

With the machine (D394), because it is a fact about which console this is - the same setting
that says retail or devkit, base or pro. And published from `orbistoun-core` rather than the
kernel, because the C library answers this one and the kernel answers the others, and those two
crates cannot see each other.

