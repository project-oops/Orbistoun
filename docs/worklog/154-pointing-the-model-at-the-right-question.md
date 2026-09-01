# Pointing the model at the right question


Three changes, each from a measurement rather than a hunch.

**It was being told the wrong libraries.** The prompt names the libraries an identifier set
belongs to, and the live run hardcoded four. Every name the model has ever earned came from
a library not among them - `libSceAgcDriver`, `libSceNpAuth`, `libSceAudio3d`. Now derived
from the round's own examples by reading the module word out of each name, which the grammar
already lists: 80% of examples resolve across 47 libraries, and all three earned names
resolve correctly. Because the examples rotate per round, the libraries rotate with them, so
each round asks about the subsystems it is actually looking at (D253).

**It had no memory of its own failures.** A round grows a local clone of the grammar,
sweeps, and discards it; only successes reach the bank. So a word that failed was proposed,
accepted and swept again every round - `Group` twelve times, at thirty-five million
candidates each. `tried_before` now holds every word swept this run, deliberately separate
from the bank, which stays evidence-only (D254).

**A third of what it proposes is already in the shipped standard list.** Nine of twenty-six
distinct words appear inside a `standard.txt` name, including three of the five it has ever
banked. Those arrive free once that list is decomposed into parts, so the prompt now says
the standard vocabulary is covered and asks for the platform's own domain nouns - which is
the two-thirds no standard name contains (D255).

**And two things deliberately not built.** Mutation and combination both looked like obvious
wins and are both jobs for a loop, not a model: a loop enumerates every variant of five
hundred words in milliseconds, and combination is what the grammar patterns already do.

