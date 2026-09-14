<p align="center">
  <img src="assets/logo.png" alt="Orbistoun" width="200">
</p>

# Orbistoun

**The Clean-Room x86-64 Native High-Level Emulator for Prospero.**

Orbistoun is a high-level emulator (HLE) for 8th and 9th generation console software (Orbis and Prospero), written in Rust. Because both the guest console and host PC share the x86-64 CPU architecture, guest code executes **natively** with zero interpreter or CPU recompilation overhead. Orbistoun's work lies entirely in the operating system layer: memory management, dynamic NID linking, thread scheduling, and translating RDNA2 GPU command streams into modern Vulkan.

Site: **[project-oops.github.io/Orbistoun](https://project-oops.github.io/Orbistoun/)**

| 📖 **[Player & Tester Guide (GUI & CLI)](docs/USER_GUIDE.md)** | ⚙️ **[Technical Reference & Architecture](docs/README.md)** |
| :--- | :--- |
| *Running titles, controller mapping, GUI walkthrough, and crash traces.* | *Address maps, RDNA2/Vulkan pipeline, ABI bridge, and blame engine.* |

---

## Role in THE LOOP

Within the [OOPS ecosystem](../docs/THE_LOOP.md), Orbistoun is the **execution and verification engine**:

```
Title Executable (from oops-apps or commercial)
         │
         ▼
┌────────────────────────────────────────┐
│ Orbistoun Native Execution             │
│ (x86-64 Native + Vulkan Graphics)      │
└──────────────────┬─────────────────────┘
                   │
         [Fault / Crash / Stub]
                   │
                   ▼
┌────────────────────────────────────────┐
│ orbistoun-turn (Automated Blame)       │
│ - Snapshot unwritten struct memory     │
│ - Arm watchpoints on empty slots       │
│ - Diff trace: FURTHER / same / BACK    │
└──────────────────┬─────────────────────┘
                   │
         [Unmeasured Question]
                   │
                   ▼
  obSCEne Hardware Oracle (PS5: 192.168.1.211)
```

1. **Native Execution**: Orbistoun maps the title into memory, resolves import NID hashes statically, and jumps to entry.
2. **Automated Blame (`orbistoun-turn`)**: When a guest faults, watchpoints identify which register or unwritten struct field triggered the crash.
3. **The Hardware Oracle**: If the function or struct is unmeasured, the loop dispatches a probe to [obSCEne](../obscene/) on physical hardware via [Prosperous](../prosperous/). Telemetry from the PS5 is converted into typed Rust structs tagged `known_by: measured`.
4. **Progress Verification**: The title re-runs. If progress is made (`verdict: FURTHER`), the implementation is promoted. If it regresses (`BACK`), it is reverted.

👉 **Read the full emulator loop specification in [docs/THE_LOOP.md](docs/THE_LOOP.md)**.

---

## Developer Quickstart

### 1. Build and Health Check
Orbistoun is developed as a sibling under the [OOPS meta-repository](../README.md):

```bash
# From repository root
./bin/orbistoun doctor --fix   # verify toolchains and fix missing dependencies
./bin/orbistoun check          # compile and run the test suite
```

### 2. Run a Title
To run a title (for example, [`gl-cube`](../oops-apps/src/gl-cube)):
```bash
./bin/orbistoun run GLCB00001
```
Orbistoun executes the guest until completion or timeout, records the trace, compares it against the previous run, and prints the verdict (`FURTHER`, `same`, or `BACK`) followed by ranked diagnostic findings.

### 3. Query Knowledge, Questions, and Worklist
Inspect what Orbistoun knows, what rests on empirical measurement, and what remains an open question:

| Command | Purpose |
|---|---|
| `orbistoun-cli symbols` | Lists all 900+ declared system functions |
| `orbistoun-cli questions` | Lists open questions ranked by how often guest titles call them |
| `orbistoun-cli worklist` | Ranked action list of what to implement next across all runs |
| `orbistoun-cli knows <symbol>` | Displays the empirical proof and citation behind any function |
| `orbistoun-cli compat list` | Shows how far every title in the corpus has reached |

---

## Architecture & Crates

```
crates/
├── orbistoun-loader   # ELF64 / SELF container loading, TLS, and address space layout
├── orbistoun-nid      # Import hash resolution and candidate grammar generation
├── orbistoun-abi      # System V AMD64 ↔ Microsoft x64 calling convention bridge
├── orbistoun-hle      # Clean-room system service stubs (libkernel, libScePad, etc.)
├── orbistoun-gpu      # GFX10 PM4 packet processor, context registers, and queue dispatch
├── orbistoun-shader   # RDNA2 GFX10 bytecode decoder and SPIR-V recompiler
├── orbistoun-turn     # Automated blame engine, 2D argument sweep, and Escape Hatch harness
├── orbistoun-report   # Trace capture, diff comparator, and ranked findings generator
└── orbistoun-cli      # Developer command-line interface
```

---

## Strict Clean-Room Rules: No Fake Stubs

Orbistoun strictly adheres to the OOPS Clean-Room conventions:
1. **Zero Leaked Code**: Zero proprietary SDK headers, zero disassembly copying.
2. **Honest Stubs**: An unimplemented function returns an explicit unhandled status or non-zero placeholder. We **never** return fake `0` success codes to "nudge" an emulator past a crash (the *Kyty trap*), as fake stubs cause silent downstream memory corruption.
3. **The Escape Hatch**: If execution hits an architectural wall (missing GPU opcode or recompiler instruction), deadlocks in a spinloop, or regresses, the autonomous loop halts, rolls back the trial patch, and escalates to `worklog.md`.

---

## Cross-Project Links

- **[Master OOPS Front Door](../README.md)** — Collection overview and building instructions.
- **[The OOPS Loop](../docs/THE_LOOP.md)** — Master ecosystem loop specification.
- **[obSCEne](../obscene/)** — Hardware conformance probe providing empirical silicon truth.
- **[Prosperous](../prosperous/)** — Remote hardware tool deploying payloads and streaming logs.
- **[SELFish](../selfish/)** — Platform file format compiler and ELF/PKG unpacker.
- **[oops-apps](../oops-apps/)** — Conforming homebrew titles used as test fixtures.
