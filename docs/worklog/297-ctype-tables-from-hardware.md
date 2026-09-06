# 2026-09-02 - (/loop) The ctype tables, measured off hardware: table right, wall unchanged

An obSCEne hardware capture arrived mid-session carrying the `035-libc/getpctype` probe, so the loop
pivoted off the GPU work to spend it. `_Getpctype` has been a known wall since D443: it answers a
pointer the guest *dereferences*, so the placeholder is an address read through (D459).

**Built** - `orbistoun-gen ctype`, a generator subcommand beside `constants` (same shape: external
source in, a crate's `data/` file out; deliberately not in the `tables` verb, which re-derives from
in-repo recordings). It emits `crates/orbistoun-libc/data/ctype.toml` - three tables, 272 `u16`
entries each, `known_by = "measured"`. `orbistoun-libc/src/ctype.rs` installs a table into guest
memory once and answers the address of entry zero, which is the *middle* of the allocation because
`table[-1]` must be readable.

**Two findings, both from the capture checking itself.** The probe records the tables twice - raw
bytes, and spot-checks taken through the *running* library - and the generator refuses to emit a table
that disagrees with them (three negative tests hold that guard down).

1. **It rejected the first run.** `table_raw_neg16` means sixteen **bytes**, not sixteen entries: the
   margin is eight `u16`s. Read as entries, index 0 lands half a table away and every classification
   is wrong while the file still reads perfectly. The alignment was then *solved* - one offset
   satisfies all twelve spot-checks at once - rather than nudged until it passed.
2. **The measured layout is not FreeBSD's** (D468). Derived by asking which characters carry which
   bit: `0x01` hex, `0x02` upper, `0x04` space, `0x08` punct, `0x10` lower, `0x20` digit, `0x40` the
   whitespace controls, `0x80` control. FreeBSD's `_CTYPE_D` is `0x400`, which in the measured table
   is **the tab bit**. D448's plan - transcribe the documented table - would have made `isdigit` true
   for tab and false for digits, silently. So: a FreeBSD-derived *kernel* does not license assuming a
   FreeBSD *C library*, and that is worth carrying beyond this table.

**The wall did not move.** PPSA02664 still faults at `image+0xb14be3`, `read of 0x7fff00cf`, verdict
`same`, **six runs in six** - deterministic, not the D450 race. This is not progress and is not what
D450 predicted either (it expected the wall to move to the tlsf allocator; it moved nowhere).

**The open puzzle, with the evidence.** Instrumentation (since removed) established all of:

- the NID matches - guest imports `0xecb8a81684f543b1`, `orbistoun-cli symbols` declares the same;
- it resolves and **binds**: `_Getpctype -> slot 54`, and `install_handlers` runs once with
  `len=1018 bound=571 slot54=Some(true)`;
- the implementation **is called** - 415 times in one run, each answering a real table pointer;
- and the run *still* faults with `rax=0x7fff0001` at `image+0xb14be3`.

So one route to this symbol reaches the implementation and another, at the faulting call site, lands
on a stub. Two routes to one symbol is the leading hypothesis (import slot vs the by-name/`dlsym`
thunks - this guest makes ~141k `sceKernelDlsym` calls, D365); it is **not diagnosed**, and the next
session should start by finding which route `0x400000b14be3` takes rather than by re-verifying the
table.

**A correction worth recording**, because it is the exact trap `CLAUDE.md` names. Mid-investigation I
read repeated "implementation called" prints as the fix working. They were not evidence of that: the
calls and the fault coexist, and the verdict line said `same` the whole time. Counting successes is
not checking for failures - the assert belongs on the failure.

Housekeeping: `cargo clippy --tests` and `fmt` clean across gen/libc/service; gen 77 + libc 92 tests
pass; `~/.oops-identity/scan.sh` clean (the capture is referenced by repo-relative name only, never by
an absolute path). Nothing committed - inside the no-commit window.

**Deferred, still available:** the same capture holds 5,400 NID→kernel-vaddr pairs
(`140-oracle/kexport-table`), a naming oracle for `orbistoun-names/data/vendor.toml`, untouched. The
GPU packed-format work is paused at 5 of 7 (UINT/SINT/UNORM/SNORM/FLOAT16 done and GPU-verified,
worklogs 292-296); SRGB is next and needs the GLSL ext-inst plumbing D467 deferred.
