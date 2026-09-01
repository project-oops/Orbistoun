# 2026-09-01 (/loop) - libSceUlt mutex/condvar/ulthread; PPSA28061 wall is now online (D435)


(kernel) New `ult` module: libSceUlt mutex family + condition-variable family onto sync.rs (a per-cond
map holds the bound mutex, since ULT binds at create). `_sceUltUlthreadCreate` = created-not-scheduled
(cooperative; no scheduler yet, entry not run - documented gap, not a fake). New libSceUlt guest_module
+ symbols.rs modules() (11->12) + knowledge file + EMBEDDED.

FINDING: these were the game's Save Thread & kin, NOT the JSON init's blocker - PPSA28061 stayed at 60
imports/abort. The real blocker is `libSceNpCppWebApi::0xa9721c01ca796f63` (unnamed, called once) - the
online NP leaderboard client. So PPSA28061's remaining wall is the online subsystem (matches the prior
note), a network/policy gap, not a local implementation. Left for a decision rather than stubbed.
kernel/service/hle tests green; kernel clippy-clean; no regression on the other two titles.

