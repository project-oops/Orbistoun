# 2026-08-31 (later) - the census-control leak, root-caused and fixed (D418)


Chased the 900-surface/control failure the user flagged. It was not a hash collision and not the
probe's fault: `symbols/generated.json` literally contained `obs_census_control_absent`, harvested
by the string miner from `titles/obscene/eboot.bin` - obSCEne's own canary name, scanned out of its
own binary. So orbistoun could name the control's NID, `named` mode didn't refuse it, and it
reported present. Removed the entry from generated.json (JSON re-validated) and taught the harvester
(`orbistoun-names::strings`) to reject `obs_`-prefixed candidates - obSCEne's private namespace,
never a platform symbol - so it cannot leak again. Added a test.

Verified: under `named`, obs_census_control_absent is now absent and 900-surface/control passes;
015-sync/machine-kind passes too (D392's sibling); 005-generation/detect becomes honest. The surprise
worth keeping: the derivation records in generated.json are what made this diagnosable in seconds -
the `from`/`by` fields pointed straight at obSCEne's eboot. Provenance accounting earning its keep.
orbistoun-names tests pass (60), clippy clean.

