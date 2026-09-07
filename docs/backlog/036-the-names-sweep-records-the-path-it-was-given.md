# The names sweep records the path it was given

A `module-strings` derivation in `symbols/generated.json` carries `from`: the module the
string was read out of, so `audit --verify-harvest` can re-read it. It is written exactly as
the sweep was invoked. Run against `titles/`, that is `titles/obscene/eboot.bin`; run against
the tool's own library in the platform data directory, it is an **absolute host path**, in a
committed file.

The identity guard caught it on 2026-09-07 (worklog 421) and the sweep was re-run through the
launcher, which now resolves `titles/` first. That is a convention holding where a rule should:
a record's path should be relative to the corpus root it was found under, and the audit should
take the root as an argument rather than trust the spelling. Until then, sweep through
`./bin/orbistoun names`, never with an absolute path.
