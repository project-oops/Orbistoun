//! A resolution on the bus is ingested only for what the rows of its cited log show: every code
//! and function its outcome names is looked for among the `OBS|` rows (D840).

use orbistoun_probe::resolution::{Unbacked, resolutions, unbacked};

const WORKLOG: &str = "\
# Worklog

### REQ-20261012T1822Z-b1ca - libSceNpEntitlementAccess: Initialize
- **status:** RESOLVED (obSCEne 2026-10-14)
- **resolved:** 2026-10-14 on live PS5 (report `reports/hardware/report-batch.log (SHA-256 `1dd1`)`)
- **from:** Orbistoun (oracle-diff, 2026-10-12)
- **to:** Obscene
- **wants:** `sceNpEntitlementAccessInitialize(param)`; the oracle says 0x80553804.
- **outcome:** delivered
  - `sceNpGetState` returns rc `0x0`.
  - `sceNpEntitlementAccessInitialize` returns `0x80550001` offline.
  - `sceSaveDataInitialize3` refuses with `0x809f0001`.

### REQ-20261013T0100Z-zz01 - an open one
- **status:** OPEN
- **from:** Orbistoun

### REQ-20261013T0200Z-zz02 - one from another project
- **status:** RESOLVED
- **from:** OopsMesa
- **outcome:** done, `0x80550001`
";

const LOG: &str = "\
<118>[SceShellCore] FMEM sceNpGetState is mentioned outside a row
OBS|measure|000-boot/np|sceNpEntitlementAccessInitialize|bound|0x0|bool
OBS|measure|000-boot/np|sceNpEntitlementAccessInitialize|status|0x0|not-possible
OBS|measure|000-boot/sd|sceSaveDataInitialize3|rc|0x809f0001|code
";

#[test]
fn only_resolved_requests_from_the_asker_are_read() {
    let found = resolutions(WORKLOG, "Orbistoun");
    let ids: Vec<&str> = found.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["REQ-20261012T1822Z-b1ca"]);
    assert_eq!(found[0].logs, ["reports/hardware/report-batch.log"]);
}

#[test]
fn the_outcome_stops_before_the_request_s_own_fields() {
    let found = resolutions(WORKLOG, "Orbistoun");
    assert!(found[0].outcome.contains("0x80550001"));
    assert!(
        !found[0].outcome.contains("0x80553804"),
        "the oracle's code in `wants` is not a claim of the outcome"
    );
}

#[test]
fn a_claim_no_row_holds_is_unbacked() {
    let found = resolutions(WORKLOG, "Orbistoun");
    let claims = unbacked(&found[0].outcome, LOG);
    assert_eq!(
        claims,
        [
            Unbacked::Absent("sceNpGetState".into()),
            Unbacked::NeverBound("sceNpEntitlementAccessInitialize".into()),
            Unbacked::Absent("0x80550001".into()),
        ]
    );
}

#[test]
fn a_claim_the_rows_hold_is_backed() {
    let claims = unbacked("`sceSaveDataInitialize3` answers `0x809f0001`", LOG);
    assert!(claims.is_empty(), "{claims:?}");
}
