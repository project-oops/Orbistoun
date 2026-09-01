# 2026-09-01 - found PPSA02664 regression (1541->234); mechanism understood, fix deferred (net-zero turn)


Went to work PPSA02664's _Getpctype wall and found it had regressed: D443 reached 1541 calls, now walls at
234 (the tlsf allocator, image+0xafcc08). Root mechanism (traced): sceKernelReserveVirtualRange has a
learned region policy that plants the policy-region base into its argument 0 - which is the guest's own
out-pointer - so the guest reserves that base, collides with orbistoun's own reservation of the region,
falls back to the high arena, and its arena-relative allocator underflows (D449). Two speculative fixes
(implemented-supersedes-region; honour-a-reserved-hint) were tried and reverted - the first removes the
load-bearing plant, the second returns the same range for the guest's two reserves. Why D443 didn't hit
this and now does is unresolved (possibly a shared-data change by a concurrent session, which was
restructuring obSCEne's docs at the time). Turn left net-zero: both attempts reverted, no stray debug,
obSCEne still runs its full suite. Fix direction recorded in D449 for a careful (non-loop) turn.
