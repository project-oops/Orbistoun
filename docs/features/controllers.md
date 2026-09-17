# Controllers

Guest pad state (`libScePad`), the host-side key/button mapping, and what is and is not
wired up yet.

`orbistoun-input` is HLE, not a wrapper over a host gamepad library: it has no dependency on
SDL2, `gilrs`, or any other input crate — see its `Cargo.toml`. It models opening and
closing a pad, handle validity, and vibration/the light bar (accepted and discarded rather
than driven — see below), plus a host-side key-to-button mapping named by button **position**
(`South`, `East`, `L1`, `R2`, …), never by a vendor glyph, per the collection's naming
convention. Up to `MAX_PORTS` (4) pads are modelled; an empty port is a title-visible,
ordinary state, not an error.

**What is deliberately not implemented yet.** The calls that read pad state *into guest
memory* — `scePadReadState`, `scePadReadStateExt`, `scePadRead`, `scePadReadExt` — are
declared, so a trace can name them, and refuse rather than answer. The structure they write
has a size and layout nobody here has measured, and inventing bytes a title would read is
exactly the fake-stub failure CLAUDE.md principle 3 forbids. So **no title currently receives
real button input through this path**, host controller or keyboard. Vibration and the light
bar are similarly accepted and discarded, not driven — there is no working haptics today.

---

## Keyboard fallback

The GUI's own input layer maps host keys to guest button positions. A working default
mapping, matched to the DualSense layout a title expects:

- `D-Pad`: Arrow Keys
- `Cross (X)`: Enter / Space
- `Circle (O)`: Escape
- `Square ([])`: X
- `Triangle (/_\)`: C
- `L1 / R1`: Q / E
- `Options`: F1

Re-mapping a key is a settings-file change (`orbistoun-input::mapping`), round-tripped
through TOML.
