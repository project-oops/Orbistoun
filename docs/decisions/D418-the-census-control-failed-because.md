# D418 - The census control failed because obSCEne's canary leaked into our symbol database


**measured** - 2026-08-31

D392 left `900-surface/control` failing even under `named`, with the honest note that "the symbol
it uses is one this build *can* name" - and punted it as a question for the probe. It was not the
probe. `symbols/generated.json` literally contained `obs_census_control_absent`, and its own
derivation record said where it came from:

```json
"obs_census_control_absent": { "found": "static", "by": "module-strings",
                               "from": "titles/obscene/eboot.bin", "on": "2026-08-25" }
```

The string harvester (`orbistoun-names::strings::candidates`) scans a corpus module for
identifier-shaped runs and offers them as candidate names. obSCEne is a corpus module too, and its
own eboot carries the string `obs_census_control_absent` - the name of the deliberately-absent
census control. Harvesting it put a **non-symbol** in the database, so this build could name the
control's NID, so `named` did not refuse it, so it resolved to a stub and reported *present*. Not a
hash collision (a 64-bit one against 30k names is ~10^-15); a name leak.

Two-part fix. The entry is removed from `symbols/generated.json`, and the harvester now rejects any
candidate beginning `obs_` - obSCEne's private prefix, which no platform library exports - so it
cannot re-enter on the next regeneration. The tell is the namespace, and it is exact.

Under `named` the control now passes (`obs_census_control_absent` absent), and D392's two siblings
move with it: `015-sync/machine-kind` passes, and `005-generation/detect` becomes honest (the
previous generation's driver resolves, the current one's does not - correct for a ps4-mode module
run) rather than claiming both. The 33 partials under `named` are libraries orbistoun genuinely
lacks, now reported absent instead of falsely whole - which is the census working.

`named` stays non-default (`all`'s stub-everything is the safe mode for *running* a guest, and every
`compat/` measurement was taken under it); this fix is what makes `named` tell the truth when a
conformance run asks for it. Recorded `measured`: confirmed by the control flipping on re-run.

