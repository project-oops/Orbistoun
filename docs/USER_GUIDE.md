# Orbistoun Player & Tester Guide

Welcome to the **Orbistoun** user and tester guide.

This guide provides practical instructions for **players, compatibility testers, and homebrew developers** running titles, configuring controllers, navigating the GUI, and reporting execution traces.

If you are an AI coding agent, compiler architect, or low-level systems engineer seeking virtual memory address maps, Vulkan translation pipelines, ABI bridge definitions, or decision records, see the **[Technical Reference](README.md)**, **[ADDRESS_MAP.md](ADDRESS_MAP.md)**, and **[THE_LOOP.md](THE_LOOP.md)** instead.

---

## Table of Contents

1. [Honest Compatibility State & Requirements](#1-honest-compatibility-state--requirements)
2. [Quickstart: Running Titles via CLI](#2-quickstart-running-titles-via-cli)
3. [`orbistoun-gui` Desktop Walkthrough](#3-orbistoun-gui-desktop-walkthrough)
   - [Game Library & Dashboard](#a-game-library--dashboard)
   - [Call Trace & Execution Inspector](#b-call-trace--execution-inspector)
   - [Memory & Register Viewer](#c-memory--register-viewer)
   - [Display & Graphics Settings](#d-display--graphics-settings)
4. [Controller Configuration](#4-controller-configuration)
5. [Reading Reports & Differential Verification](#5-reading-reports--differential-verification)
6. [Submitting Traces to the Corpus](#6-submitting-traces-to-the-corpus)
7. [Troubleshooting & Common Questions](#7-troubleshooting--common-questions)

---

## 1. Honest Compatibility State & Requirements

### The Current Reality
- **Commercial Retail Games**: **Do not boot or display gameplay yet.** No emulator anywhere currently runs retail commercial PS5 titles.
- **What Runs Today**: Self-contained homebrew (`gl-cube`), graphical test suites, and hardware conformance probes (`obSCEne`). All local test corpus executables load, link, resolve NIDs, and execute real native guest x86-64 machine code.

### Minimum System Requirements:
- **OS**: Windows 10/11 (64-bit) or x86-64 Linux.
- **CPU**: x86-64 CPU supporting AVX2 and BMI2 (Intel Haswell / AMD Zen 1 or newer).
- **GPU**: Vulkan 1.3 compatible graphics card (AMD RDNA/GCN, NVIDIA Turing/Ampere, Intel Arc).
- **RAM**: 8 GB minimum (16 GB recommended).

---

## 2. Quickstart: Running Titles via CLI

**The binary is `orbistoun-cli`** — there is no bare `orbistoun` executable. Point `run` at a
title's `eboot.bin` (created by `selfish --format title` or dumped from hardware):

```powershell
# Run a title's eboot.bin
orbistoun-cli run C:\Games\GLCB00001\eboot.bin

# Run with detailed debug output
OOPS_LOG=debug orbistoun-cli run C:\Games\GLCB00001\eboot.bin

# Run with trace-level per-syscall logging
OOPS_LOG=trace orbistoun-cli run C:\Games\GLCB00001\eboot.bin
```

From a source checkout, `./bin/orbistoun run <title-id>` is the development wrapper: it
resolves a title id under `titles/`, rebuilds, refreshes names if stale, and runs it in one
step — see [WORKFLOW.md](WORKFLOW.md).

---

## 3. `orbistoun-gui` Desktop Walkthrough

Launch the desktop interface with:
```powershell
orbistoun-gui
```

### A. Game Library & Dashboard

Manage your installed title directories, view game icons, and launch with one click:

```text
+-------------------------------------------------------------------------------+
|  Orbistoun - Next-Generation Console Emulator                    [_][O][X]    |
+-------------------------------------------------------------------------------+
| File  Emulation  View  Debug  Help                                            |
|-------------------------------------------------------------------------------|
| [Add Title Dir...]  [Refresh Library]  [Settings]  [Stop Emulation]           |
|-------------------------------------------------------------------------------|
| Icon    | Title ID   | Title Name                   | Category | Compatibility|
|---------+------------+------------------------------+----------+--------------|
| [ICON]  | GLCB00001  | GL-Cube 3D Demo (Stage 2)    | BIG_APP  | ENTERED      |
| [ICON]  | OBSC00001  | obSCEne Hardware Conformance | BIG_APP  | FLIPPED      |
| [ICON]  | WIPE00001  | WipEout Model Viewer         | BIG_APP  | ENTERED      |
| [ICON]  | PPSA02664  | Commercial Title A           | BIG_APP  | FLIPPED      |
+-------------------------------------------------------------------------------+
| Status: Idle | Vulkan: AMD Radeon RX 6700 XT | Backend: Native x86-64         |
+-------------------------------------------------------------------------------+
```

Compatibility labels are the reach terms from [COMPATIBILITY.md](../COMPATIBILITY.md)
(`rejected` / `parsed` / `linked` / `entered` / `flipped`) — "flipped" means a frame reached
the output layer, not that anything was drawn; see [graphics.md](features/graphics.md).

*(Screenshot placeholder: Game Library & Dashboard)*

---

### B. Call Trace & Execution Inspector

When running in debug mode, inspect live system calls and HLE resolutions as they occur:

```text
+-------------------------------------------------------------------------------+
|  Execution Inspector - GLCB00001                                 [_][O][X]    |
+-------------------------------------------------------------------------------+
| Step | Function / Symbol Name        | Known By | Return Code | Latency       |
|------+-------------------------------+----------+-------------+---------------|
| 0001 | sceKernelVirtualQueryInfo     | assumed  | 0x00000000  | 0.04 ms       |
| 0002 | sceKernelAllocateDirectMemory | published| 0x00000000  | 0.12 ms       |
| 0003 | sceKernelMapDirectMemory      | assumed  | 0x00000000  | 0.08 ms       |
| 0004 | sceAgcDriverCreateQueue       | guest-observed | 0x00000000 | 0.45 ms  |
| 0005 | sceAgcSubmitDcb               | measured | 0x00000000  | 0.22 ms       |
+-------------------------------------------------------------------------------+
| [Pause Execution]   [Step Into]                                              |
+-------------------------------------------------------------------------------+
```

`Known By` is one of `published` / `measured` / `guest-observed` / `assumed` — see
[CLAUDE.md](../CLAUDE.md) principle 1. Most recorded behaviour today is `assumed`; `measured`
is the minority, earned from a hardware probe, not the default.

*(Screenshot placeholder: Call Trace & Execution Inspector)*

---

### C. Memory & Register Viewer

Inspect the native x86-64 host context and guest virtual memory layout:

```text
+-------------------------------------------------------------------------------+
|  Memory & Register State                                         [_][O][X]    |
+-------------------------------------------------------------------------------+
| RAX: 0000000000000000  RBX: 0000000800402000  RCX: 0000000000000038           |
| RDX: 00007fffffffe120  RSI: 00007fffffffe100  RDI: 0000000800400000           |
| RSP: 00007fffffffe0c0  RBP: 00007fffffffe0f0  R8 : 0000000000000000           |
| RIP: 0000000000401140 (gl-cube.elf: main + 0x140)                             |
|-------------------------------------------------------------------------------|
| Virtual Memory Range:                                                         |
|   0x0000000000400000 - 0x0000000000600000 : Main Executable (RX)              |
|   0x0000000800000000 - 0x0000000880000000 : Direct Memory / AGC Ring Buffer    |
|   0x00007fffff800000 - 0x00007ffffffff000 : Main Thread Stack (RW)            |
+-------------------------------------------------------------------------------+
```

*(Screenshot placeholder: Memory & Register State)*

---

### D. Display & Graphics Settings

The Vulkan device orbistoun would present to, shown for information rather than
configuration — **the rendering backend itself is still a stub**: every draw and present
call is refused by name today, so nothing here changes what appears on screen yet. See
[graphics.md](features/graphics.md) for the honest current state.

```text
+-------------------------------------------------------------------------------+
|  Orbistoun Settings: Graphics                                    [_][O][X]    |
+-------------------------------------------------------------------------------+
| Vulkan device:      [ AMD Radeon RX 6700 XT (RADV)                          v]|
| Presentation:        not implemented - every draw/present call is refused     |
+-------------------------------------------------------------------------------+
```

*(Screenshot placeholder: Graphics Settings Menu)*

---

## 4. Controller Configuration

Controller support is HLE: `orbistoun-input` reimplements guest-visible `libScePad` state
rather than wrapping a host gamepad library, so what is supported is whatever the GUI's own
input layer maps for you, not a vendor SDK. See [controllers.md](features/controllers.md).
- **Keyboard Fallback**:
  - `D-Pad`: Arrow Keys
  - `Cross (X)`: Enter / Space
  - `Circle (O)`: Escape
  - `Square ([])`: X
  - `Triangle (/_\)`: C
  - `L1 / R1`: Q / E
  - `Options`: F1

---

## 5. Reading Reports & Differential Verification

Orbistoun emphasizes honest failure. When an implementation is missing, it refuses to fake success:

```powershell
orbistoun-cli report C:\Games\GLCB00001\eboot.bin
```
`report` surveys the module, persists a run report, and prints the delta against the
previous run for the same title — reached state, unresolved-import count, and (from the
second run on) how many imports became newly resolved or newly unresolved.

**The `FURTHER` / `same` / `BACK` progress verdict is part of a `run`'s own output**, printed
by `orbistoun-cli run` (or `./bin/orbistoun run <title>`) each time, not a separate command:
- `FURTHER`: The title executed guest code it could not reach before.
- `same`: Identical execution path — the expected result on an unchanged tree.
- `BACK`: A regression — the run stopped earlier than the previous one.

`orbistoun-cli verify <path>` is a different, narrower question: how much of a module's
import list the current symbol database can name (`N of M imports named`) — useful before
debugging a title with mostly-unnamed imports, not a progress comparison.

---

## 6. Submitting Traces to the Corpus

If you encounter an unhandled call or crash while testing:
1. Just run the title — every run writes a call trace to disk on both the fault and
   time-limit paths, with no separate flag needed:
   ```powershell
   orbistoun-cli run C:\Games\my_title\eboot.bin
   ```
2. Run `orbistoun-cli paths` to see exactly where trace artifacts land on this machine.
3. Submit the trace log in an issue to help ground our next `obSCEne` probe!

---

## 7. Troubleshooting & Common Questions

### Q: The title crashes with `Refused: unimplemented symbol [name]`
- **Explanation**: Orbistoun encountered an API that has not yet been probed on hardware. Rather than returning dummy values that silently corrupt state 40,000 frames later, Orbistoun stops honestly.
- **Resolution**: This finding enters our oracle loop to be probed on real PS5 silicon.

### Q: There is no rendered window at all
- **This is currently expected**, not a misconfiguration. The Vulkan backend
  (`orbistoun-gpu-vulkan`) still refuses every draw and present call by name — see
  [graphics.md](features/graphics.md) — so no title produces a picture yet, only guest
  execution and (where wired up) GPU compute dispatch. Check the run's own log for whether
  the title reached draw submissions (`sceAgcSubmitDcb`) even though nothing is shown.

