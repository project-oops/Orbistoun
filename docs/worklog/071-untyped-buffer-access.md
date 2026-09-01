# Untyped buffer access


`buffer_load_dword` and `buffer_store_dword` translate and run: the descriptor is read from
four scalar registers, the addressing equation is emitted as arithmetic, and all four
out-of-bounds modes are evaluated because the selector is a runtime value. Five device
tests - round trip, past the end, the last access that fits, the indexed path, and a
swizzled descriptor.

**7 of 10 fixtures complete, 112 of 127 instructions.** 613 workspace tests green.

### Surprises

- **A test helper put 256 into an eight-bit field** and the spare bit landed in the field
  next door, quietly moving a load's destination from v2 to v3. It presented as "the load
  returns zero", and the descriptor and the stored memory both looked perfect - the store
  had worked, the load had worked, and the value was in a register nobody was reading. The
  same shape as the too-narrow-field faults in the solver, one layer up.
- **The bounds check had to be built for a runtime selector.** Which of the four modes
  applies is two bits of a register, so all four are evaluated and selected between. That
  is not a shortcut - it is what "the descriptor is data" actually costs.

### Outstanding

The typed accesses (`tbuffer_*`) convert between memory formats on the way through; that
format table is a separate piece. `exp`, VINTRP and MIMG still need render targets and
fragment inputs, which is G11.

