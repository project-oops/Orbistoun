# D910 - A fetch from a later layer reads the array whole

**Status:** decided
**Date:** 2026-10-20

A dispatch may address a view of a 2D array that starts past its first layer. A view of one later
layer L alone is that layer's level, placed as a colour target's slice is (D859, D899): L chains on
from the base, and in a 64 KiB `_X` mode with the layer's pipe-bank XOR. `64KB_S` has no layer
terms (`tools/addrlib/equation 64KB_S 4 32 array`). Both a fetch and a store take it.

A view of several layers from a later one is fetched by reading the array from its first layer and
dropping the layers before the view. A store to such a view is refused.

**Why:** Grand Theft Auto V's loading-screen program fetches layer one of a two-layer colour array
and stores to layer one of its `10_11_11_FLOAT` cube.
