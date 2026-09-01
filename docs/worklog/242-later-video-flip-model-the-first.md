# 2026-08-31 (later) - video flip model: the first complete obSCEne run in orbistoun (D424)


sceVideoOutSubmitFlip / SetFlipRate / GetFlipStatus were declared but unregistered, so obSCEne's
display path polled a flip that never completed and faulted at ~29% (image+0x5d72af). Implemented the
flip queue as a per-port counter that completes on submit (no scanout, no vblank to wait for), with
GetFlipStatus writing the count at offset 0 - the one citable field. Registered all three (video was
already wired into service::symbols). Video tests + clippy clean.

Result, measured immediately: 158→543 unique tests, 29→38 sections, OBS|end reached - the first
end-to-end obSCEne run inside orbistoun. 515 pass / 10 partial / 4 fail / 14 skip under the default
resolver; the 4 fails + 10 partials are the next fidelity targets, now that the suite completes. This
closes the arc the session opened with (stale corpus → sandbox → payload → flip): orbistoun can now
run the current obSCEne test-for-test against a hardware report.

