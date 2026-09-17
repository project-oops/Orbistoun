# Orbistoun User Guide

Cross-cutting reference for how Orbistoun behaves under the hood — system requirements, honest compatibility status, paths and portable mode, save data, and crash reporting. Per-feature pages live alongside; this page addresses cross-cutting questions.

---

## Honest Compatibility State

### The Current Reality
- **Commercial Retail Games**: **Do not boot or display 3D gameplay yet.** No emulator anywhere currently runs commercial retail PS5 titles.
- **What Runs Today**: Self-contained homebrew (`gl-cube`), graphical test suites, and hardware conformance probes (`obSCEne`). All local test corpus executables load, link, resolve NIDs, and execute real native guest x86-64 machine code.

### Minimum System Requirements
- **OS**: Windows 10/11 (64-bit) or x86-64 Linux.
- **CPU**: x86-64 CPU supporting AVX2 and BMI2 (Intel Haswell / AMD Zen 1 or newer).
- **GPU**: Vulkan 1.3 compatible graphics card (AMD RDNA/GCN, NVIDIA Turing/Ampere, Intel Arc).
- **RAM**: 8 GB minimum (16 GB recommended).

---

## Paths and Portable Mode

Orbistoun persists titles, saves, settings, and logs to a single data root resolved at startup.

### Default Mode: `%APPDATA%\OOPS\` (Windows) or `~/.local/share/OOPS/` (Linux)
Shared across all OOPS projects, so a save directory pulled from physical hardware by Prosperous lands immediately in the tree Orbistoun mounts.

```text
%APPDATA%\OOPS\ (Windows) or ~/.local/share/OOPS/ (Linux)
    titles/             <- Staged titles and guest filesystems
    saves/              <- Encrypted and decrypted title save files
    reports/            <- Hardware and emulator run reports
    screenshots/        <- Captured frames
    config.toml         <- User emulator settings
```

### Portable Mode

Drop a `.portable` directory (or sentinel file) next to the executable, or set
`ORBISTOUN_PORTABLE_MODE=1`:

```text
<wherever you put it>/
    orbistoun-cli.exe   (or orbistoun-gui.exe)
    .portable           (sentinel directory or file)
    titles/             (stored right beside the binary)
    saves/
    config.toml
```

In portable mode, **zero data is written to the host user profile**. Everything stays self-contained on a USB stick or portable directory. Portable mode is also automatically activated if the executable name contains `portable` (e.g. `orbistoun-portable.exe`).

---

## Data & Privacy

Orbistoun is strictly **local-first**:
- **Zero Telemetry**: Orbistoun never phones home. No telemetry, no usage tracking, no remote error uploads.
- **No Online Accounts**: No account login or cloud DRM.
- **Your Saves are Plain Files**: Saved game states and overlays are stored directly on your disk for easy backup and transfer.

