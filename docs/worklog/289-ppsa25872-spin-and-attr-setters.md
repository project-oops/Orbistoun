# 2026-09-02 - (/loop) PPSA25872's unnamed spin (naming-gated) + pthread attr setters

Spread off PPSA04263 to PPSA25872 (Unity/il2cpp). Its wall (`image+0x7b594e`, null read) is the same
*shape* as PPSA04263's WaitEventFlag spin: `PS5Util::0xf948d02a4f9f5ace` is 91% of 39,929 calls
(36,495) against a placeholder. But two things make it harder, and neither is oracle-free:

- **It is unnamed.** The deterministic name search already ran and left this hash uncracked, so
  re-running it finds nothing new; cracking it needs new candidate vocabulary (the `suggest` model, or
  the Unity PS5 SDK symbols), not another sweep.
- **The guest ignores its return value.** Forcing it to answer `0` via `ORBISTOUN_RETURN=
  0xf948d02a4f9f5ace:0x0` changed nothing - still 91% spinning. So the guest is polling a **side
  effect**: the function takes a pointer (arg0 = `0x740000754a38`, a mapped struct) and the guest
  loops reading what it should *write* there. Without the function's contract, what to write is a
  guess, and guessing a side effect is exactly the honest-failure trap. This one wants a name (model/
  hardware) or an obSCEne trace, not more deterministic crunch.

While there, cleared four unimplemented attr setters flagged across PPSA04263/PPSA21564:
`scePthreadAttrSet{schedpolicy,inheritsched,affinity,guardsize}`. These are **honest**, not accept-
and-lie: the attr object is an 8-word block with a store/read pair (`attr_set`/`attr_get`), and offsets
16-48 were free, so each stores its value where a matching get reads it back - the setter's whole
contract. Affinity is stored, not applied (orbistoun does not pin guest threads; documented).
clippy/fmt/tests clean; no regression - PPSA04263 (28,343) and PPSA21564 (500,257) unchanged.

**State of the overnight crunch.** The readily oracle-free work is largely mined out. Tonight took
PPSA04263 from a 32-call death to deep guest logic through four structural fixes (map-into-reservation
D460, MAPPING_BASE collision D463, per-thread TLS D464, blocking WaitEventFlag D465) plus the return-
value and reserve-failure diagnostics (D459, D462) and a run of standard-library stubs. What remains is
gated on things this loop cannot do without data: the two `int 0x41` guest asserts (PPSA04263,
PPSA21564) want the assert's read traced to the value we got wrong; PPSA25872's spin wants a name;
`_Getpctype` wants the ctype table off hardware (obSCEne); PPSA28061 wants the GPU. Next tick will
finish the last clearly-standard stubs (`vsprintf_s`, `sceKernelSetVirtualRangeName`) and then the loop
is at the honest edge of what "doesn't need data" can reach.
