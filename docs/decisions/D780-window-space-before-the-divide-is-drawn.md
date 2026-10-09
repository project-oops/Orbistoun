# D780 - Window space before the divide is drawn under the window-space viewport

**Status:** decided
**Date:** 2026-10-09
**known_by:** published (`gfx103.json` `PA_CL_VTE_CNTL`: the six scale and offset enables, and
`VTX_XY_FMT` / `VTX_Z_FMT` saying whether x, y and z arrive already divided by `W`)

A draw whose `PA_CL_VTE_CNTL` has every field orbistoun reads at zero has its positions in window
space before the divide. There is no viewport scale or offset, and the hardware divides x, y and z
by the fourth component, so the pixel is `(X / W, Y / W)` and the depth `Z / W`. Its primitive
shader writes `(X / S, Y / S, Z, W)`, and the draw takes the fixed window-space viewport D731
uses, scale `S` = `WINDOW_SPACE_SCALE` and offset zero. Vulkan's divide and scale then give back
the same pixel and depth. D731's form, radeonsi's pre-divided one with `1/W` in the fourth
component, is unchanged.

**Why:** PPSA02664's fullscreen passes write `PA_CL_VTE_CNTL` zero, and every such draw was refused
as a viewport transform turned off.
