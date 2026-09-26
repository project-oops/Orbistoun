# D444 - Flexible memory is a separate budget from the direct-memory pool

**Status:** decided
**Date:** 2026-09-01

The flexible-memory size queries answer from their own configured and available figures, not
from the direct-memory pool's size; both figures are system-wide defaults.

**Why:** A conformance probe measures both a configured total and an available figure for
flexible memory that are far smaller than the direct pool, and every resident title's own launch
parameters carry no override for either figure - so the measured defaults are the figures every
title launches under, not one probe's private budget.

**Rejected:** answering the direct-memory pool's size for a flexible-memory query - measured an
order of magnitude high; treating the measured figure as one probe's private budget rather than
the shared default - contradicted by every title's own launch parameters carrying no override.
