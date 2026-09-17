# 665. Two corpus-reached AGC builders `-4059` missed: `ResetQueue` wired, `WaitUntilSafeForRendering` refused

**2026-09-17** — inbox `-b7e4` re-filed `-4059` (worklog 663) because its resolution over-claimed:
"the corpus-reached builders are all wired" was not true. Two reached names —
`sceAgcDcbResetQueue` and `sceAgcDcbWaitUntilSafeForRendering` — were in neither `implementations()`
nor the refusal doc. This closes the gap honestly: one wired, one refused, and the stale count that
`-4059` read as evidence corrected.

## The over-claim, and why worklog 663 didn't catch it

Worklog 663 fixed a stale *refusal* sentence in `agc.rs` and read the resolution as "coverage met".
It never re-ran the reached-vs-implemented cross-reference, so two reached builders that had sat
measured since 2026-09-10 stayed invisible. That is the honest-failure principle one level up: a
resolution reported more coverage than it measured. `-b7e4`'s cross-reference
(`comm -23 reached.txt impl.txt`) is the measurement worklog 663 should have run.

## `sceAgcDcbResetQueue` — wired as a reservation skeleton

The measurement (`166-agc/dcb-reset-queue`, sweep `20260910-174437`) is the whole 32-byte
writer-struct: `0xffff1000` (the NOP filler `sceAgcCbNop` emits) then two `SET_UCONFIG_REG` packets
(`0xc0027904`, `0xc0017904`) writing the command-processor marker register `0x342` — the same
headers `marker_skeleton`/`wait_reg_mem_skeleton` already emit. So it wired the same way they did:
`packet::build::reset_queue_skeleton` reproduces the framing and the 32-byte extent, `dcb_reset_queue`
appends it through the writer handle, and `tests/dcb_wiring.rs` pins it.

The body is **zeroed**, not reproduced, and the reason is provenance. obSCEne's probe
(`check_agc_dcb_reset_queue`) calls the builder with **all-zero arguments** (`fn_reset(&begin, 0, 0,
…)`), yet two body dwords come back `0xce200000` and `0xcea00000` — address-shaped and *differing
between the two packets*, the fingerprint of a pointer into obSCEne's own writer struct rather than a
constant. A single zero-argument capture cannot separate a fixed value from the probe's address, and
baking the latter into every guest's stream is the exact defect `set_cx_registers_indirect_skeleton`
exists to avoid. The reservation is the load-bearing part anyway: D559 records PPSA02664 calling
`ResetQueue` on its writer twice before any other AGC use, so a real cursor advanced by 32 bytes is
what clears the wall the placeholder held.

## `sceAgcDcbWaitUntilSafeForRendering` — refused, and why not a no-op

`-b7e4` suggested wiring it as "a builder that appends nothing and says so", reading obSCEne's
`fail (wrote 0)` as a measured empty encoding. Reading the actual sweep result (`-a6aa`,
`20260915-125124`) does not support that: it is labelled `fail`, exactly like `DcbSetFlip: fail
(wrote 0)` and the SIGSEGV crashes — a probe failure. The sweep *does* show what a measured zero
looks like, one line up: `CbSetShRegistersDirect: partial 0 B (GetSize 0, wrote 0)` — the library
itself answering a zero reservation. `WaitUntilSafeForRendering` has no `GetSize 0`, so its encoding
is simply unknown, and a no-op built on it would be plausible output resting on a failed
measurement. It is refused in the doc block above `implementations()`, citing the measurement, and a
re-probe (capture it under a queue prepared by `ResetQueue`, since a wait/sync builder writing zero
on a bare writer is most likely state-gated) is filed to obSCEne as `REQ-...4e91`.

## The count

`agc.rs`'s module doc still opened "Eight of the command builders are now wired" — the numeral
worklog 663 corrected the refusal sentence beside but left, and the number `-4059` read as coverage.
The prose no longer states a count (it would go stale as builders land, 44 → 45 here); the count is
pinned by `tests/dcb_wiring.rs`, which now asserts 45.

## Gate state

`cargo clippy -p orbistoun-gpu --all-targets` clean; `tests/dcb_wiring` 16 passed; gpu lib 73 passed;
fmt clean; `./bin/orbistoun prose` exit 0; identity scan clean. No commit.
