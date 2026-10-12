# D818 - A reference emulator is a prediction, never a source

**Status:** decided
**Date:** 2026-10-12

What another emulator does enters orbistoun in one place only: as a **prediction** inside an
obSCEne request. It never enters a knowledge entry, a `known_by`, a comment that states a
fact, or code. A request carrying predictions resolves only on obSCEne's own sweep rows that
reproduce or refute each predicted value; a resolution that quotes the prediction back is not
a resolution, and the arms it missed are re-filed. The behaviour then enters orbistoun as
`measured`, citing the rows, exactly as if no prediction had existed.

Every refuted prediction is recorded in `crates/orbistoun-hle/data/oracle-ledger.toml`: the
request, the area, the predicted value and the measured one. The ledger's refutation rate per
area says where a prediction is worth sending as the first arm of a probe, and where it is
not.

**Why:** a reference emulator plays more titles than orbistoun does, so it is the cheapest
checklist of what a booting title needs, and its answers make good first arms for a probe.
They are also wrong often enough to matter: the census of 2026-10-12 found answers already
contradicted by hardware in SaveData, CommonDialog, Ajm, AvPlayer and Json2, eight of them
with rows the ledger cites. Copying an answer would import those errors with no record of where they came from,
and a reference emulator's licence can forbid carrying its code, structures or tables across
(D003). Used as a prediction, an answer costs nothing when it is wrong: the probe refutes it
and the ledger records it.

**Rejected:**
- Treating an answer two emulators agree on as `published`: agreement between two
  implementations is one guess repeated, not a source.
- Ignoring reference emulators: the census they made possible found 693 imports and a dozen
  GPU layers nobody had asked about.
- Taking a prediction as the implementation and probing later: the loop keeps only verified
  code ([THE_LOOP](../THE_LOOP.md#properties-of-the-loop)).
