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
    let claims = unbacked(&found[0].outcome, &[LOG]);
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
    let claims = unbacked(&found[0].outcome, &[LOG]);
    let decisive: Vec<bool> = claims.iter().map(Unbacked::is_decisive).collect();
    assert_eq!(decisive, [false, true, true]);
}

/// A section the outcome cites whose own set-up failed - a call answering -1 - and that reports
/// measurements after it, measured nothing: ix01's stored vertex ids followed
/// `queue-create-rc 0xffffffff`. A section that stops at its failure is a `not-possible`, not a
/// claim.
#[test]
fn measurements_after_a_failed_set_up_are_unbacked() {
    let rows = concat!(
        "OBS|measure|000-boot/ids|setup|fn-create-queue-bound|0x0|bool\n",
        "OBS|measure|000-boot/ids|gpu|queue-create-rc|0xffffffff|code\n",
        "OBS|measure|000-boot/ids|arm1|stored-id-0|0x0|id\n",
        "OBS|measure|000-boot/np|sceNpThing|bound|0x0|bool\n",
        "OBS|measure|000-boot/np|sceNpThing|status|0x0|not-possible\n",
    );
    let outcome = "measured in `000-boot/ids` and `000-boot/np`: ids `{0, 1, 2}`";
    assert_eq!(
        unbacked(outcome, &[rows]),
        [Unbacked::AfterFailedSetUp(
            "000-boot/ids: queue-create-rc 0xffffffff, then stored-id-0".into()
        )]
    );
}

/// A section is judged within each log it appears in: ix02 cited an older sweep whose run of the
/// section failed its set-up and a newer one whose run succeeded, and the newer one's measurements
/// do not come after the older one's failure.
#[test]
fn a_failed_set_up_in_one_log_does_not_taint_another() {
    let older = "OBS|measure|000-boot/ids|gpu|queue-create-rc|0xffffffff|code\n";
    let newer = concat!(
        "OBS|measure|000-boot/ids|setup|queue-create-rc|0x0|code\n",
        "OBS|measure|000-boot/ids|arm1|stored-id-0|0x0|id\n",
    );
    assert_eq!(unbacked("in `000-boot/ids`", &[older, newer]), []);
}

/// A row an outcome quotes must be in the log it cites, word for word: gp02 quoted
/// `fn-create-queue-bound|0x1` where its log holds `0x0`.
#[test]
fn a_quoted_row_the_log_does_not_hold_is_unbacked() {
    let rows = concat!(
        "OBS|measure|000-boot/q|setup|fn-create-queue-bound|0x0|bool\n",
        "OBS|measure|000-boot/q|setup|handle|0x9|handle\n",
    );
    let outcome = concat!(
        "  - rows:\n",
        "    ```\n",
        "    OBS|measure|000-boot/q|setup|handle|0x9|handle\n",
        "    OBS|measure|000-boot/q|setup|fn-create-queue-bound|0x1|bool\n",
        "    ```\n",
    );
    assert_eq!(
        unbacked(outcome, &[rows]),
        [Unbacked::QuotedRowAbsent(
            "OBS|measure|000-boot/q|setup|fn-create-queue-bound|0x1|bool".into()
        )]
    );
}

#[test]
fn a_claim_the_rows_hold_is_backed() {
    let claims = unbacked("`sceSaveDataInitialize3` answers `0x809f0001`", &[LOG]);
    assert!(claims.is_empty(), "{claims:?}");
}
