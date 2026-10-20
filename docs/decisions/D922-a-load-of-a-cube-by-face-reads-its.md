# D922 - A load of a cube by face reads its slices as an array

**Status:** decided
**Date:** 2026-10-20

A pixel shader may load from a depth cube with `dim:2D_ARRAY` and name the face as the layer
coordinate. The six slices drawn as the cube's faces (D918) then bind as the layers of one 2D
array, and the backend sees them through an array view, not a cube view. The view must name the
whole cube, starting at the first face.

This is `published`: an image instruction reads a cube resource as a 2D array, and Mesa issues
every image load of a cube that way (`ac_shader_util.c:589-591`, `ac_get_image_dim`).

**Why:** Grand Theft Auto V's loading-screen lighting loads one face of a point light's shadow
cube with `image_load ... dim:2D_ARRAY` through user data 8. Bound neither as a cube nor as a
first layer, the slot was refused, and its draw segment never ran.

**Rejected:**
- Binding the cube view and turning the layer back into a direction in the shader. The module
  already declares an arrayed image (D897). An array view gives it that image, and nothing has to
  be rebuilt.
