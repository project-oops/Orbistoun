# 719. PPSA02664's wall is external data, confirmed against obSCEne's delivered measurements

**2026-09-19** — asked to keep hardening PPSA02664 (Alex Kidd in Miracle World) with accurate emulation and no
hacks. This tick cross-referenced the wall against obSCEne's *delivered* findings and ran one more
experiment. The conclusion is unchanged from worklogs 716/717 but now rests on obSCEne's own
measurements rather than orbistoun's inference, and one remaining hypothesis is closed.

## What obSCEne has already settled (delivered, cross-referenced this tick)

The wall is the unnamed `libSceAgc::0x7d86501b8094ef57`, whose unfilled workload object faults at
`+0xa8` in Unity's `CreateWorkload`. obSCEne's inbox answers three requests about it:

- **`-7c21` (delivered):** NID `0x7d86501b8094ef57` is **not an export** of retail `libSceAgc.sprx`
  on FW 12.40. A forward hash search over 834,780 mined names produces no collision in either
  reading. Its explicit recommendation: *"Orbistoun should not wait for a vendor symbol name that
  does not exist in the retail export table. Orbistoun can dispatch the NID directly by NID …
  matching the observed call shape `(ptr, ptr, 0, ptr, u32, u32)` with command buffer in arg1."*
- **`-e245` / `-0681` (delivered):** a packaged title that declares this NID as an ordinary
  (`GLOBAL FUNC`) import has it **bound to NULL** by the retail loader, precisely because libSceAgc
  does not export it.
- **`-7a5e` (delivered):** libSceAgc *can* be mapped natively now (native PS5 eboot,
  `TARGET=prospero`), which retires the old "no leg maps libSceAgc" blocker - but `sceAgcCreateShader`
  returns `0x8a6c002f` in that leg, so it does not, by itself, open the workload object.

This reframes the block precisely: **naming was never the requirement - orbistoun binds by NID.**
What is required is the object's *layout*, and that is unobtainable: the function is an internal,
unexported routine, so it cannot be called or dumped on hardware, it is absent from the title's own
files (no firmware), and it does not collide with any known name.

## What the function is, from the evidence

We do not have its name, but its **role** is legible from how it is called - stated as inference,
kept apart from the facts above:

- Its result is passed as `arg0` to **`sceAgcSetCxRegIndirectPatchAddRegisters` thirty-two times**
  immediately after the call, and there is a measured **`sceAgcDcbSetCxRegistersIndirectGetSize`**
  companion. So it is the **producer of the "set context-registers indirect" family**: the builder
  that creates the indirect-register descriptor which `GetSize` sizes and the `Patch*` functions
  fill with register entries.
- Context registers can be written inline or *indirect* (a packet pointing at a separate buffer of
  `(register, value)` pairs, patched afterward) - general PM4/AGC structure orbistoun already models
  for the direct form. The `+0xa8` field the guest dereferences off a null base is most plausibly
  **the pointer to that value buffer** the producer should have allocated and did not, because it is
  unimplemented.
- It is very likely an **AGC SDK inline/helper emitted out-of-line** by this Unity build rather than
  a true `.sprx` routine, which is exactly why the retail loader binds it NULL (the SDK expected it
  compiled into the caller, so `libSceAgc.sprx` never exported it). Its exported, named sibling
  `sceAgcDcbSetCxRegistersIndirect` is the one already in the knowledge base.

Knowing the role does not immediately unblock it: the exact packet body and descriptor offsets are
unmeasured - the whole indirect-register family (`SetCxRegistersIndirect`'s body plus every
`Patch*`) is absent from every hardware census, and this producer cannot be called to be dumped.

## The experiment this tick, and why it is a dead end

If retail binds the import to NULL and the title still runs, one hypothesis was that orbistoun's
non-null placeholder is what sends the guest down the faulting path. Tested by matching retail:
`ORBISTOUN_RESOLVE=named ./bin/orbistoun run PPSA02664-app0` refuses every import this build cannot
name (which includes this NID). Result: the image is left with ten unresolved relocations and the
run **halts before entry** ("not entered - the image is not fully linked", D010). So refusing the
unnamed set en masse cannot isolate this one import, and orbistoun has no by-NID null-bind that
still enters. Even if it did, a direct `GLOBAL` call to a null address faults at the call, earlier
than today's `+0xa8`, not further - the guest does not null-check a non-weak import. The
null-binding hypothesis is closed: it cannot be tested cleanly and would not move the wall.

## Two routes past it: one forbidden, one legitimate

The routes that are exhausted or forbidden:

- **Name it** (does not exist), **measure its bytes on hardware** (unexported, uncallable),
  **match retail's null binding** (untestable in isolation, and a direct call to null faults
  earlier not further). All closed.
- **Blindly invent the descriptor layout** to fill `+0xa8` - the plausible-output hack principle 3
  and the request ("no hacks") both forbid.

But there is a route that is neither, and it is worth separating from blind invention: **rebuild the
indirect-register mechanism from the guest oracle.** orbistoun would own all three sides - the
producer, the `Patch*` functions, and the translator that reads the result at submission - so it can
define one self-consistent representation in which nothing is invented:

- the register `(offset, value)` pairs are the **guest's own**, taken from the 32 `PatchAddRegisters`
  argument sets, not guessed;
- the context-register writes they describe are applied through the **already-measured direct-register
  path** (`sceAgcDcbSetCxRegisterDirect`), so the GPU effect is the measured one;
- the only thing that must match the retail struct is the handful of **descriptor offsets the guest
  itself reads** (`+0xa8` and any others), and those are *measured from the guest* by poison-probing
  which offsets it dereferences - principle 3's "the guest itself is an oracle", not invention.

The risk that keeps this honest: if the guest touches the descriptor in ways beyond a value-buffer
pointer, or the `Patch*` argument semantics do not map cleanly to `(register, value)` pairs, the
representation stops being faithful and the effort must stop rather than paper over it. So it is a
hard, multi-step guest-oracle reconstruction with a real chance of hitting its own wall - but it is a
legitimate accurate path, not the blind fill worklogs 716/717 (rightly) refused.

**The bounded next step** is measurement, not code: poison-probe (`ORBISTOUN_WRITE`) which offsets of
`arg0`/`arg3` the guest reads and writes around this call, to map the descriptor the guest expects
before building anything to fill it.

### Probing the producer's args and return - all four ruled out

The fault is a **`memcpy(dst = 0x74…, src = NULL, count = 0xa8)`** - the guest copies a 168-byte
structure *from* a pointer that is null. `dst` (`rax`/`rcx`) is a guest heap object that varies per
run (`0x74…21cf070`, `0x74…1acf070`, `0x74…228f070`), so the src is stored in some object the
producer should have populated. Four `ORBISTOUN_WRITE`/`ORBISTOUN_RETURN` runs tested whether the
producer's args or return feed that src; **none moved the fault** (all `same`, still `read of 0xa8`,
`r13=r14=0`):

- **Return, a valid mapped address** (`ORBISTOUN_RETURN=…:0x400000000000`, not just `0x0`): unchanged.
  This closes the "the guest null-checks an error-looking return and zeroes it" hypothesis - a valid
  return is ignored too, so the return is genuinely not the src (strengthening worklog 716).
- **`arg0[0x00..0xa0]`, every 8 bytes, poisoned** (all planted): unchanged. The src is not read from
  the producer's writable stack struct anywhere in its first 168 bytes.
- **`arg1` and `arg3`**: the plants were **refused** - in PPSA02664's actual call those args do not
  point at writable memory, so they are not the out-param either.

So the producer's influence on the null src is **not through its arguments or its return**. What an
implemented `0x7d86501b8094ef57` must do is a **global/heap side-effect** - allocate the 168-byte
source structure and register its pointer somewhere the guest re-loads later - which poisoning the
call's args cannot simulate or reveal. The guest-oracle *arg* probe has reached its limit here, and
in doing so it partly de-attributes the story: the null src is not something the producer hands back
or writes into a caller-supplied buffer.

### The next method: disassemble the src load

The remaining honest lever is to read the guest's own code (a lawful oracle - the title's bytes, not
a reference emulator): disassemble around the `memcpy` call site (`0x400000042ebd`) and trace which
object and offset the `src` register is loaded from. That names where the null originates - which may
turn out to be a different unimplemented function than `0x7d86501b8094ef57`, or a global the producer
should register. orbistoun-cli has no disassembler, so this needs external tooling and a SELF→vaddr
offset mapping; it is the next tick's setup.

**Separately, fidelity work that does not need the wall to move:** the same reservation-skeleton
builders carry measured headers but zero bodies, waiting on further obSCEne argument-pass
measurements - "the same APIs the real device provides", but not a reach change.

## Gate state

No code changed - a cross-reference, one experiment run, and a reading. `./bin/orbistoun check` was
green as of worklog 718; nothing here touches code. Identity scan clean.
