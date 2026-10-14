//! A resolution on the bus is ingested only for what the rows of its cited log show: every code
//! and function its outcome names is looked for among the `OBS|` rows (D840).

use orbistoun_probe::resolution::{Unbacked, resolutions, unbacked};

const WORKLOG: &str = concat!(
    "# Worklog\n",
    "\n",
    "### REQ-20261012T1822Z-b1ca - libSceNpEntitlementAccess: Initialize\n",
    "- **status:** RESOLVED (obSCEne 2026-10-14)\n",
    "- **resolved:** 2026-10-14 on live PS5 (report `reports/hardware/report-batch.log ",
    "(SHA-256 `1dd1`)`)\n",
    "- **from:** Orbistoun (oracle-diff, 2026-10-12)\n",
    "- **to:** Obscene\n",
    "- **wants:** `sceNpEntitlementAccessInitialize(param)`; the oracle says 0x80553804.\n",
    "- **outcome:** delivered\n",
    "  - `sceNpGetState` returns rc `0x0`.\n",
    "  - `sceNpEntitlementAccessInitialize` returns `0x80550001` offline.\n",
    "  - `sceSaveDataInitialize3` refuses with `0x809f0001`.\n",
    "\n",
    "### REQ-20261013T0100Z-zz01 - an open one\n",
    "- **status:** OPEN\n",
    "- **from:** Orbistoun\n",
    "\n",
    "### REQ-20261013T0200Z-zz02 - one from another project\n",
    "- **status:** RESOLVED\n",
    "- **from:** OopsMesa\n",
    "- **outcome:** done, `0x80550001`\n",
);

const LOG: &str = concat!(
    "<118>[SceShellCore] FMEM sceNpGetState is mentioned outside a row\n",
    "OBS|measure|000-boot/np|sceNpEntitlementAccessInitialize|bound|0x0|bool\n",
    "OBS|measure|000-boot/np|sceNpEntitlementAccessInitialize|status|0x0|not-possible\n",
    "OBS|measure|000-boot/sd|sceSaveDataInitialize3|rc|0x809f0001|code\n",
);

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
            Unbacked::Unnamed("sceNpGetState".into()),
            Unbacked::NeverBound("sceNpEntitlementAccessInitialize".into()),
            Unbacked::Absent("0x80550001".into()),
        ]
    );
}

#[test]
fn a_function_no_row_names_is_not_decisive_alone() {
    let found = resolutions(WORKLOG, "Orbistoun");
    let claims = unbacked(&found[0].outcome, LOG);
    let decisive: Vec<bool> = claims.iter().map(Unbacked::is_decisive).collect();
    assert_eq!(decisive, [false, true, true]);
}

#[test]
fn a_claim_the_rows_hold_is_backed() {
    let claims = unbacked("`sceSaveDataInitialize3` answers `0x809f0001`", LOG);
    assert!(claims.is_empty(), "{claims:?}");
}
