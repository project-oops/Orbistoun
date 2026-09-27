# D731 - Window-space positions are drawn through a fixed viewport

**Status:** decided
**Date:** 2026-09-27

A draw whose `PA_CL_VTE_CNTL` is the value radeonsi gives a window-space shader - no viewport
scale or offset, x, y and z already divided, the fourth component `1/W` taken as is
(`si_state_shaders.cpp:1321-1322`; Gallium's `VS_WINDOW_SPACE_POSITION`,
`docs/gallium/tgsi.rst:3705-3711`) - runs its primitive shader as a module that writes
`(x W / S, y W / S, z W, W)` for `W = 1/q`, under a viewport of scale `S` and offset zero on both
axes, `S` being `WINDOW_SPACE_SCALE`, 8192. Vulkan's divide and viewport take that back to the
pixel the guest named. Any other value with the x/y terms off is refused by name, as before.

**Why:** radeonsi's blits write pixel positions; Vulkan has no window-space position, only clip
space, so the translation and the viewport between them have to cancel. A power of two makes the
scaling exact, and one fixed transform means one module per shader rather than one per target
size. The window-space form belongs to the shader in radeonsi, so a module per (shader, position
space) costs nothing where it is not used.

**Limits:** a position more than `S` pixels from the origin falls outside the clip volume and is
clipped; so is a depth outside `[0, 1]`, which the hardware, with clipping disabled, would not
clip. A device whose `maxViewportDimensions` or `viewportBoundsRange` cannot take the viewport
refuses the draw by name.

**Rejected:**
- A viewport the size of the target and positions mapped into it: the module would depend on the
  target's size.
- Drawing the positions as clip space: they land somewhere else entirely.
