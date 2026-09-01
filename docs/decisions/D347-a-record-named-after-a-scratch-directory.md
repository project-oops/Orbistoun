# D347 - A record named after a scratch directory


**decided** · 2026-08-27 · found by listing `compat/` instead of assuming what was in it

Recording became automatic (D323), and the title is derived from the module's containing
directory. So running a payload that happened to sit in an Explorer default produced a
**tracked** record:

```
compat/New folder (2).toml
  outcome = "0x7ff61e175ee3"   imports = 3   standing = 34%
```

A host address rather than `image+...`, three imports, describing a loader payload rather
than a title - filed in the directory whose whole purpose is the compatibility claim this
project makes about titles.

**Automatic recording made a latent looseness into pollution.** While a person typed
`compat record`, the directory name was their problem and they would not have typed it. Taking
the person out removed the judgement that had been silently doing the filtering, which is a
cost of automation worth naming rather than a bug in the automation.

A title id is an identifier: letters, digits, dot, dash, underscore. Anything else is a
directory name, and the run says so rather than skipping quietly - a run that declines to
record and a run with nothing to record must not look the same (principle 3).

**Found by listing the directory.** Not by a test, not by the gate - by checking a claim
about the state of the tree rather than reciting it, after being wrong twice that day doing
the reverse.

