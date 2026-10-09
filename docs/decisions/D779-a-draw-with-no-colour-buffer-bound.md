# D779 - A draw with no colour buffer bound writes depth alone

**Status:** decided
**Date:** 2026-10-09
**known_by:** published (`gfx103.json` `ColorFormat` `COLOR_INVALID` = 0; radeonsi writes it for an
empty colour slot, `si_state.c:2720-2724`)

A submission whose `CB_COLOR0_INFO.FORMAT` is `COLOR_INVALID` has no colour buffer bound. Its draws
run with their colour going nowhere: the drawer keeps their depth, which is never written back
because the guest's depth tiling is unmeasured. The colour the drawer produces is discarded.
Any frame pending for a target is written back first. The drawer's colour is then forgotten, so
the next colour draw starts from what guest memory holds.

**Why:** PPSA02664 draws depth passes with colour target zero left `COLOR_INVALID`. They were
refused as a target no frame can be written back to, and nothing after them retired. Drawing them
against a target of their own extent would put depth-pass colour where a frame is kept.
