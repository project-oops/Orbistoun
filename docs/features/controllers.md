# Controllers

Gamepad mapping, DualSense haptics, and keyboard emulation.

Orbistoun provides plug-and-play controller support via SDL2, mapping physical controllers directly to guest `libScePad` structures.

---

## GUI: Controller Mapping

Open **Settings → Input** from the top menu bar.

```text
+-------------------------------------------------------------------------------+
|  Orbistoun Settings: Controller Mapping                          [_][O][X]    |
+-------------------------------------------------------------------------------+
| Controller Port:    [ Port 0 (Active User)                                  v]|
| Device:             [ Sony Interactive Entertainment DualSense Wireless     v]|
| Type:               [ PlayStation DualSense (USB/BT Native)                 v]|
|-------------------------------------------------------------------------------|
| Button Mapping:                                                               |
|   Cross (X)    : Button 0 (A)        Square ([])  : Button 2 (X)              |
|   Circle (O)   : Button 1 (B)        Triangle (/\): Button 3 (Y)              |
|   L1 / R1      : Shoulder Buttons    L2 / R2      : Analog Triggers (0-255)   |
|   Left Stick   : Analog Axes 0, 1    Right Stick  : Analog Axes 2, 3          |
|   Touchpad     : Trackpad Click      Options      : Menu Button               |
+-------------------------------------------------------------------------------+
| [ Calibrate Deadzones ]   [ Test Vibration ]   [ Reset Defaults ]             |
+-------------------------------------------------------------------------------+
```

![Orbistoun Controller Mapping](screenshots/controllers.png)
*(Screenshot placeholder: Controller Mapping)*

### Supported Devices:
1. **PlayStation DualSense & DualShock 4**: Full native button and analog trigger resolution over USB or Bluetooth.
2. **Xbox / XInput Gamepads**: Automatically mapped with ABXY translation.
3. **Keyboard Fallbacks**:
   - `D-Pad`: Arrow Keys
   - `Cross (X)`: Enter / Space
   - `Circle (O)`: Escape
   - `Square ([])`: X
   - `Triangle (/_\)`: C
   - `L1 / R1`: Q / E
   - `Options`: F1

