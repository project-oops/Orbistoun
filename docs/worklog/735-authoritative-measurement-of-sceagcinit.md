# 735. authoritative measurement of sceagcinit clears a3f0 and confirms createworkload wall

**2026-09-20** — Resolution of obSCEne ticket `REQ-20260920T1425Z-a3f0`. The empirical measurement of
`sceAgcInit` on live PS5 console hardware (FW 12.40) has landed, disproving the theory that `sceAgcInit`
populates an AGC context buffer at `arg0`. The verified behavior is wired into Orbistoun, CI gates pass,
and PPSA02664 confirms clean advancement into `CreateWorkload` where the `read of 0xa8` wall remains.

## 1. Hardware measurement on PS5 (FW 12.40)

To resolve ticket `a3f0`, obSCEne's probe check `166-agc/init` was updated with three multi-buffer passes:
1. **Sentinel buffer**: A 1024-byte buffer pre-filled with `0xAA` sentinels passed to `sceAgcInit(buf, 13)`.
2. **Descriptor pattern**: A buffer matching PPSA02664's initial stack descriptor (`00 00 00 00 01 00 00 00`
   followed by `0x55` sentinels).
3. **Pointer-to-pointer query**: A `void **out_ptr` initialized to NULL passed as `arg0`.
4. **NULL pointer validation**: `sceAgcInit(NULL, 13)`.
5. **Version sweep**: Sweeping version argument `arg1` from 0 to 64.

The probe was compiled as native eboot `PPSA90000`, deployed to target hardware via `pros restore`,
executed via `pros launch`, and captured from `klogsrv :3232` via `pros logs`.

### Empirical results:
- **Zero bytes written**: Across all passes, `extent = 0` and `changed = 0`. Not a single byte of `arg0`
  was touched. `out_ptr` remained NULL (`out-ptr-valid 0x0`).
- **NULL resilience**: `rc-null = 0x0`. Passing a NULL pointer succeeded without faulting.
- **Version gate**: Version 13 (`0xd`) returned `0x0` (OK). All other versions returned `0x8a6c0004`
  (`SCE_AGC_ERROR_INVALID_VERSION`).

### Refutation of the "State Buffer" theory:
Worklog 731 observed that blind return-forcing of `sceAgcInit` to `0x0` regressed PPSA02664 into an earlier
read fault (`0xf7ff0039` in `image+0x3ac99`), and hypothesized that `sceAgcInit` must populate an initial
state buffer at `arg0` that downstream calls dereference. Hardware measurement has thoroughly refuted this:
`sceAgcInit` does **not** populate a state buffer. It is a pure library version initialization gate.

## 2. Orbistoun wiring and knowledge ingestion

1. **Implementation (`crates/orbistoun-gpu/src/agc.rs`)**:
   - Implemented `agc_init(args)` validating that `args[1] as u32 == 13 ? OK : 0x8a6c_0004`, leaving `arg0`
     untouched.
   - Bound both the canonical export `"sceAgcInit"` and the raw import alias `"0x53bbd82b51d172db"` (arity 2)
     in `guest_module!` and `implementations()`.
   - Added unit tests `agc_init_validates_version_and_touches_no_state` and integration tests in `dcb_wiring.rs`.

2. **Knowledge Ingestion (`crates/orbistoun-hle/data/knowledge/libSceAgc.toml`)**:
   - Added documentation for raw import alias `0x53bbd82b51d172db` and updated `sceAgcInit` citing `a3f0`
     and the measured version gate/zero-extent writes.

3. **Status and Gate Sync**:
   - Updated wired builder count assertion in `tests/dcb_wiring.rs` to 49.
   - Ran `orbistoun-cli status --write` and `orbistoun-cli compat markdown`.

## 3. Runtime verification on PPSA02664

Running `PPSA02664` with the new implementation:
- `0x53bbd82b51d172db` called 1x, cleanly returning `0x0`.
- The title advanced past `sceAgcInit` without regressing to `0xf7ff0039`.
- The guest reached `flipped`, resolving 222 distinct imports (+1) and executing 418,342 calls.
- Execution reached `todo: void GfxDevicePS5SharedData::CreateWorkload()` and faulted at `read of 0xa8`
  in `VCRUNTIME140.dll+0x1dc8d` inside `libc::memcpy` (`r13 = 0x0`, `r14 = 0x0`).

This authoritatively confirms that `PPSA02664` is still at the `CreateWorkload` null container wall,
and proves that the missing container object is not produced by `sceAgcInit`.

## 4. Gate state

Full `./bin/orbistoun check` run via Git Bash:
- Provenance guard: OK
- Harvested constants: OK
- Decision and worklog numbers: unique
- Status numbers and compat markdown: current
- Symbol audit: OK
- Generated tables: OK
- Hardware assertions: OK
- `cargo fmt --all -- --check`: OK
- `cargo clippy --workspace --all-targets -- -D warnings`: OK
- `cargo check --workspace --all-targets`: OK
- `cargo test --workspace`: all 100+ tests pass (0 failures)
- `cargo doc --workspace`: OK
- `cargo-deny` (advisories, bans, licenses, sources): OK

All gates green. No unapproved git commits.
