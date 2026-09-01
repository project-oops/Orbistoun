# Shader translation assistance

Vendor shader bytecode to SPIR-V is pattern-heavy and has a genuine oracle in
framebuffer diffing, which makes it the one layer where automated translation can be
checked rather than trusted. Blocked on the GPU layer existing at all (phase 6).

