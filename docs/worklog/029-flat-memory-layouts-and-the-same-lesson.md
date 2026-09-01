# 2026-08-20 - Flat memory layouts, and the same lesson learned twice


**Done.** Reworked the flat memory probes; all fifteen opcodes solve, and the two that
matter most now decode their scalar base.

**Verified.** Gate green. `global_store_dword` and `global_load_dword` carry three
operands each - address, data or destination, and the base - solved from six samples.

**Surprises.** Both are the same mistake seen from two angles, and both were already
written down in this file before being made again.

- **`global_load_dword` solved its address field one bit too narrow.** No probe used a
  register above 110, so a seven-bit field explained every sample of an eight-bit one.
  That is verbatim the failure documented when the solver was built - and it happened
  while *adding probes to the file that carries the warning*. Writing a lesson down
  does not apply it; only the probe set does.

- **The `off` form hides the scalar base entirely.** It is not printed as an operand, so
  solving from it yields a layout with no field for the base - and a translator built on
  that layout would silently ignore a base address rather than refuse one it cannot
  handle. The fix was to stop probing that form: solve only from samples that name a
  base, and the no-base case then decodes as a base whose value a translator can
  recognise and reject.

  Worth generalising: **a probe set that omits a case teaches the solver that the case
  does not exist.** Absence in the samples becomes absence in the table, and nothing
  downstream can tell that apart from a field that genuinely is not there.

**Not done.** Still no guest memory buffer, so every load and store is decoded and
refused. The top five worklist entries wait on it, and it is the next unit: a second
storage binding, an address-to-index rule, and a check that refuses a base register the
translator cannot yet honour.

