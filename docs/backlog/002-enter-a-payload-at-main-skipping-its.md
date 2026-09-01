# Enter a payload at `main`, skipping its runtime start


**A one-bit experiment nobody has run, and it may retire the only research problem on
[PAYLOADS.md](../PAYLOADS.md).**

All five open-toolchain payloads die in `__crt_start` at a `ud2`, rejecting the handoff
structure orbistoun cannot supply (D308). But `main` is a real, sized, `GLOBAL FUNC` symbol
in three of them - klogsrv (685 bytes), shsrv (217), ftpsrv (615) - so it can be located by
name without deriving anything.

If `__crt_start` is what unpacks the structure and calls `main(argc, argv)` - the ordinary
shape - then entering at `main` sidesteps the whole problem for those three. klogsrv's
`DT_INIT_ARRAY` is empty, so there is nothing being skipped that runs constructors.

Costs an entry-address override, which `EntrySettings` has no field for yet. **Decisive
either way**: if the guest starts calling imports, three payloads are unblocked and the
sweep is only needed for the two stripped ones. If it dies the same way, `main` itself takes
the structure, which is worth knowing before spending boots on a sweep.

