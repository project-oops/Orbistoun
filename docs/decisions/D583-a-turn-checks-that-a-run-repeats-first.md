# D583 - A turn checks that a run repeats first

**Status:** decided
**Date:** 2026-09-08

Every plan `orbistoun-turn` makes opens with `Step::CheckRepeats`: two runs of the guest with
nothing applied, compared on where it died and how far it got. When they disagree the turn says
that every comparison below measures that disagreement as well as its own intervention.

**Why:** every mechanical step is a difference between two runs, and a guest that varies on its
own puts its variation into that difference. The check is a precondition of reading the turn, so
it heads the plan rather than being ranked among findings. It compares the signals the other
steps compare, so it refuses only variation they could notice. One disagreement is enough to
know.

**Rejected:**
- Trusting the baseline: attributes a guest's own drift to the intervention.
- Ten runs: ten boots to raise confidence in a negative that changes nothing.
- Ranking it among findings: everything ranked above it is read before the warning.
