# D823 - A three-dimensional image is sampled as a host volume

**Status:** decided
**Date:** 2026-10-13

A guest sample of a three-dimensional image (`dim:SQ_RSRC_IMG_3D`) translates to a host binding
declared `Dim3D`, sampled at its three address elements - across, down and deep - as the guest
gave them, with no conversion. The texture source it reports says `volume`, beside `cube` and
`compare` (D811, D816), and one image read in two shapes is refused as before.

The binding reads the guest's image as a host three-dimensional image of its slices. Until that
read exists, a volume source binds nothing: a flat image or a cube under a module that samples
three coordinates would not be the image the module declares, so the draw is refused at the
binding rather than at translation. The slices' layout is measured: a linear 3D image's slices lie
one after another at `pitch * height`, with no padding (obSCEne `REQ-20260920T1050Z-5d7c`,
`20260921-final-eboot.obs.log` 16743-16831).

**Why:** gl2-probe's `texture-3d` check (GLTP00001) samples a volume, and its refusal stops the
command processor, so every check after it reads the clear colour. A volume's coordinates need
nothing like a cube's rebuilt direction: the sampler takes them as positions, as the host does.

**Rejected:**
- Sampling a volume's first slice as a flat image: it answers for one depth only, and the draw
  would look right where it is wrong.
- A depth-compare of a volume: not translated, as for a cube.
