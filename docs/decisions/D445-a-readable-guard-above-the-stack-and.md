# D445 - A readable guard above the stack, and obSCEne now runs its whole suite under orbistoun


**measured** - 2026-09-01 (user-directed /loop)

With obSCEne running as the oracle (D444), its next wall was `136-kernel/handoff`: a fault at
`image+0x4502ff` reading `0x600000801000`. The handoff probe reads twenty words through the pointer the
entry hands it (`rdi`), scanning for a kernel anchor. orbistoun sets `rdi` to the argument block it builds
at the top of the guest stack (`ImageAddress`, `rdi = 0x600000800f80`), and that block sits **flush against
the reserved stack top** - only `0x80` bytes of it before `0x600000801000`, the first unmapped byte. Twenty
words is `0xA0`, so the read ran one word past the top and faulted. On hardware the stack region extends
above the block and the same read lands on mapped memory.

Fixed in `GuestStack`: a **readable guard above the initial stack pointer** (`READAHEAD_GUARD`, one page),
mirroring the unmapped guard below it. The lower guard faults so an *overflow* is caught; the upper one is
mapped and readable so a modest *over-read* of the argument block lands on zeroes, as it does on a console,
rather than on a fault with no relation to the cause. `initial_pointer` is unchanged (still the top of the
usable stack); the guard is extra span above it. A unit test reserves a stack and reads both ends of the
guard back as zero, proving it is mapped and readable - distinct from the lower guard, which is not.

**The result is the milestone this oracle was for: obSCEne runs its entire 555-check suite under orbistoun
and reaches `OBS|end`**, where before my two fixes it crashed around check forty. The tally is `520 pass /
10 fail / 7 skip / 18` against the console's `241 / 79 / 192 / 43` - orbistoun runs many checks a sandboxed
title on hardware skips, so more *run*, and the divergences are now enumerable rather than hidden behind a
crash. (The handoff check itself still shows `skip` because obSCEne's own resume record, written when it
crashed there pre-fix, skips it to get past; it clears on a clean completion. The fix is proven by the unit
test and by the suite now completing.)

**The ten failures are the next work, and the oracle named them:**

- `020-memory/virtual-query-stack` and `virtual-query-text` (`0x80020002`) - `sceKernelVirtualQuery` only
  consults the runtime `mappings()` space, so it answers "not mapped" for the guest's own stack and image.
  This is the image/stack blindness flagged in the D444 investigation, now confirmed from two checks. One
  fix (teach the query the image/stack/TLS regions) clears both.
- `900-surface/control` - a symbol obSCEne knows does not exist was reported *present*. orbistoun's resolver
  answers for names it should not, which makes every presence count in that section meaningless. A resolver
  false-positive worth chasing.
- `110-modules/info-size` and `110-modules/names` (`0x80020016`) - the module-list gap (orbistoun presents
  one module where a title sees many).
- `135-sysctl/osrelease` and `137-kernelcall/system-version` - `sysctlbyname(kern.osrelease)` and the
  system-version call are refused; obSCEne measured both on hardware, so orbistoun can answer them from the
  report rather than invent.

Verified: `orbistoun-mem` tests pass (23, incl. the new guard test), clippy clean, fmt-clean; the cli builds
and obSCEne completes the suite.

