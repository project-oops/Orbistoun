# 755. The phantom's return gates the register-group path, and confirms the relocation wall

**2026-09-21** — servicing the request inbox's REQ-8a44 (the decisive one-command test named in 745 and
not taken) turned up more than the yes/no it asked for. The phantom `0x7d86501b8094ef57`'s **return**
value gates which path PPSA02664's workload build takes; it is **not** the base of the `0xa8` pointer;
and forcing it off the wall confirms — by moving to a further fault — that the register-group relocation
(748) is exactly what stands in the way.

## The test, and that the override matched

`ORBISTOUN_RETURN=0x7d86501b8094ef57:0xdead0000 ./bin/orbistoun run PPSA02664-app0`. The report's own
line proves the override reached the function rather than the D230 "reached nothing" trap: *"this run
was under 0x7d86501b8094ef57 answers 0xdead0000 (3 answered)"*, and the census shows
`libSceAgc::0x7d86501b8094ef57(…) -> 0xdead0000` three times. So the count is real.

## What changed, side by side

- **Unmodified** (phantom returns `OK`/`0x0`): the faulting copy is `memcpy(dst, src=0xa8, n=0x50)` in
  the copier `0x42d90`, `read of 0xa8` in `VCRUNTIME140.dll+0x1dc8d` (740-748).
- **Under the sentinel**: every `memcpy` from `0x42ebd` **succeeds** (f068…f218, each with a return),
  the register-group copy now reads `src 0x6000007fc1e0 n 0x100` from the **stack**, and the fault has
  **moved** to `image+0x3f8f0`, a `write to 0x94` (null + offset) — reached just after another unnamed
  NID `libSceAgc::0x71040c4df8235e1d(0x6000007fc1e0) -> 0xf7ff0001` answered the Unimplemented
  placeholder. The report notes the frontier moved: `fault image+0x3f8f0 (was VCRUNTIME140.dll+0x1dc8d)`.

Neither of 8a44's two framings holds literally: `0xdead00a8` (sentinel + `0xa8`) appears **nowhere**, so
the return is not the pointer base; and the source does not stay `0xa8` either, because the guest took a
**different branch** entirely. The return is a **path selector**, not an operand.

## What it means, kept to the observation

This is an intervention, so by principle 5 it is read as a dependency, not a fix: `0xdead0000` is not the
phantom's correct return (that value is unmeasurable — the NID is a non-export, obSCEne `-5e0b`
not-possible). What the move proves is that the `0xa8` wall sits on the **phantom-returns-OK path**, and
that path is the one that builds the null-based register descriptor 748 dissected — the descriptor whose
group pointer holds a raw, unrelocated `0xa8`. Since a `GetSize` returning `0x0` (success) is the
expected answer, orbistoun is on the right path and the wall is the **relocation**, precisely as 748
concluded and 751 fixed for the SDK's `sceAgcCreateShader` (PPSA02664 inlines that logic and so is not
cleared by 751 — 750). Forcing the phantom off that path only swaps the relocation wall for the next
gap on the error branch (`0x71040c4df8235e1d`, itself unnamed and unmeasurable), which is not progress
worth keeping.

So 8a44 is answered — the return gates the path, is not the base — and the answer strengthens rather
than redirects the standing diagnosis: PPSA02664's wall is the inlined register-group relocation, and it
waits on the same thing 752/753 named (a hardware probe of the header relocation, or the Unity-version
input that shapes the inlined loader), not on the phantom.

## Gate state

No code changed — one override run and analysis. `./bin/orbistoun check` unchanged from 754 (green but
for the same three generated-doc drifts from a prior session's uncommitted `compat/PPSA02664-app0.toml`
edit, not this tick). Identity scan clean. No commit.
