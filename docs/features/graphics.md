# Graphics

Vulkan 1.3 rendering pipeline, RDNA2 shader lowering, GFX10 texture detiling, and display configuration.

Orbistoun intercepts platform graphics command buffers (`sceAgcSubmitDcb`), decodes hardware PM4 packets, translates RDNA2 Wave32 compute and graphics shaders into SPIR-V, and submits them to modern host Vulkan devices.

---

## GUI: Graphics Settings

Open **Settings → Graphics** from the top menu bar.

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

![Orbistoun Graphics Settings](screenshots/graphics.png)
*(Screenshot placeholder: Graphics Settings)*

### GUI Controls:
- **Renderer Selection**: Vulkan 1.3 (Hardware) or CPU Software fallback.
- **V-Sync**: Synchronizes frame flips with host monitor refresh rate (60 / 120 Hz).
- **RDNA2 PM4 Shader Lowering**: Controls SPIR-V SSA restructuring passes for Wave32 execution.
- **Texture Detiling**: Toggles between software and hardware compute-based 4KB/64KB GFX10 detiling kernels.
- **Resolution Scale**: Scales render targets from 1.0x (native 4K) down to 0.5x (1080p performance mode).

---

## CLI: Graphics Configuration Flags

```bash
# Force specific Vulkan physical device
orbistoun run --gpu-device 0 build/title/GLCB00001

# Disable V-Sync for uncapped benchmarking
orbistoun run --no-vsync build/title/GLCB00001
```

