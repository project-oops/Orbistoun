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

Orbistoun accepts title directories (created by `selfish --format title` or dumped from hardware) or bare `eboot.bin` containers:

```powershell
# Run a title directory
orbistoun run C:\Games\GLCB00001

# Run with detailed debug output
OOPS_LOG=debug orbistoun run C:\Games\GLCB00001

# Run with trace-level per-syscall logging
OOPS_LOG=trace orbistoun run C:\Games\GLCB00001
```

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
| [ICON]  | GLCB00001  | GL-Cube 3D Demo (Stage 2)    | BIG_APP  | PLAYABLE     |
| [ICON]  | OBSC00001  | obSCEne Hardware Conformance | BIG_APP  | PASS         |
| [ICON]  | WIPE00001  | WipEout Model Viewer         | BIG_APP  | IN-GAME      |
| [ICON]  | PPSA02664  | Commercial Title A           | BIG_APP  | LOAD / FAULT |
+-------------------------------------------------------------------------------+
| Status: Idle | Vulkan: AMD Radeon RX 6700 XT | Backend: Native x86-64         |
+-------------------------------------------------------------------------------+
```

![Orbistoun Library UI](screenshots/orbistoun_gui_library.png)
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
| 0001 | sceKernelVirtualQueryInfo     | measured | 0x00000000  | 0.04 ms       |
| 0002 | sceKernelAllocateDirectMemory | measured | 0x00000000  | 0.12 ms       |
| 0003 | sceKernelMapDirectMemory      | measured | 0x00000000  | 0.08 ms       |
| 0004 | sceAgcDriverCreateQueue       | measured | 0x00000000  | 0.45 ms       |
| 0005 | sceAgcSubmitDcb               | measured | 0x00000000  | 0.22 ms       |
+-------------------------------------------------------------------------------+
| [Pause Execution]   [Step Into]   [Compare against Golden Run (D016)]         |
+-------------------------------------------------------------------------------+
```

![Orbistoun Inspector UI](screenshots/orbistoun_gui_inspector.png)
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

![Orbistoun Memory State UI](screenshots/orbistoun_gui_memory.png)
*(Screenshot placeholder: Memory & Register State)*

---

### D. Display & Graphics Settings

Configure rendering backends and presentation options:

```text
+-------------------------------------------------------------------------------+
|  Orbistoun Settings: Graphics                                    [_][O][X]    |
+-------------------------------------------------------------------------------+
| Renderer:           [ Vulkan 1.3 (Hardware Accelerated)                     v]|
| Device:             [ AMD Radeon RX 6700 XT (RADV)                          v]|
| V-Sync:             [ Enabled                                               v]|
| RDNA2 PM4 Shader:   [ SPIR-V SSA Lowering (Strict Wave32)                   v]|
| Texture Detiling:   [ GFX10 Hardware Detiler (4KB / 64KB)                   v]|
| Resolution Scale:   [ 1.0x (Native 3840 x 2160)                             v]|
+-------------------------------------------------------------------------------+
| [ Apply Settings ]   [ Restore Defaults ]   [ Cancel ]                        |
+-------------------------------------------------------------------------------+
```

![Orbistoun Settings UI](screenshots/orbistoun_gui_settings.png)
*(Screenshot placeholder: Graphics Settings Menu)*

---

## 4. Controller Configuration

Orbistoun natively supports modern gamepads via SDL2:
- **PlayStation DualSense / DualShock 4**: Plug-and-play via USB or Bluetooth.
- **Xbox / XInput Controllers**: Fully supported with automatic button mapping.
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
orbistoun report
```
Output:
```text
Title: GLCB00001 (gl-cube)
Total calls: 24
Known implementations: 24 (100% measured)
Placeholders encountered: 0
Verdict: CLEAN_EXIT (0x0)
```

To compare whether a code modification got further or regressed:
```powershell
orbistoun verify
```
- `FURTHER`: The title progressed past previous barriers without regressing other calls.
- `SAME`: Identical execution path.
- `BACK`: Regression detected; execution stopped earlier than previous run.

---

## 6. Submitting Traces to the Corpus

If you encounter an unhandled call or crash while testing:
1. Run with trace output:
   ```powershell
   orbistoun run --record-trace my_title > trace.log
   ```
2. Check `orbistoun paths` to locate trace artifacts.
3. Submit the trace log in an issue to help ground our next `obSCEne` probe!

---

## 7. Troubleshooting & Common Questions

### Q: The title crashes with `Refused: unimplemented symbol [name]`
- **Explanation**: Orbistoun encountered an API that has not yet been probed on hardware. Rather than returning dummy values that silently corrupt state 40,000 frames later, Orbistoun stops honestly.
- **Resolution**: This finding enters our oracle loop to be probed on real PS5 silicon.

### Q: The window opens but remains black
- Ensure Vulkan 1.3 drivers are up to date.
- Confirm whether the title has reached draw submissions (`sceAgcSubmitDcb`) or is currently initializing thread state.

