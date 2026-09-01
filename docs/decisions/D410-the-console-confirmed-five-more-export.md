# D410 - The console confirmed five more export vaddrs, and validated the whole base+vaddr model


Status: measured.

obSCEne's `139-exports` ran as an elfldr payload on the console and confirmed seven of eight
candidate export vaddrs by calling `base + vaddr` and checking the function behaved: getpid
`0x5b0`, sceKernelWrite `0x16e00` (already confirmed), and newly **getuid `0x630`, geteuid
`0x650`, getgid `0x870`, getppid `0x7d0`, and sceKernelGetProcessTime `0x16160`**. Those five are
promoted from candidate to `confirmed` in `data/libkernel-vaddrs.txt`, so `libkernel_provenance`
now reports them Confirmed and the `firmware` verb shows it.

The larger result is that **the layout model this crate is built on is now hardware-validated.**
Every one of those functions was reached through exactly the arithmetic D407 lays down - base is
word zero (getpid), every export sits at base+vaddr - and each behaved as itself on real silicon.
A wrong base or a wrong offset would have put the call somewhere that is not the function; none
did. The `payload_args[0] = getpid` anchoring and the vaddr table are no longer an assumption.

**sceKernelGetTscFrequency at `0x1cf30` was refuted and stays a candidate.** The subtlety is worth
recording so it is not mis-read as a bad vaddr: the offset comes from the module's own export
table (a real placement), and what failed was obSCEne's *assumption* that the function is a no-arg
getter returning the frequency - it may take a pointer argument instead. So the honest state is
"placed, behavioural contract unconfirmed" = candidate, which is what it already is. No change to
the entry; the refutation is a fact about the probe's signature guess, not about where the symbol
lives. A duplicate `getpid 0x5b0 confirmed` line was also removed.

Recorded `measured`: these are confirmations of guest-facing behaviour on hardware, not
assumptions.

