# The decision log had been quietly losing its own references


Not planned work. Reading D122 for the review queue turned up **two** entries with that
number, and then fifteen duplicated numbers overall - thirteen of them cited from source,
so those citations point at two different decisions each (D201).

`./orbistoun.sh check` fails on duplicates now. The existing fifteen are a recorded ceiling
that can only shrink.

### Surprises

- **I had contributed one that afternoon**, and the fix collided *again*: the number I
  moved to had been taken by the other session between my reading the file and writing to
  it. Two collisions in ten minutes is the clearest possible statement that "read the last
  number and add one" is not a workable protocol with more than one writer.
- **The check is more valuable than fixing the backlog.** Renumbering fifteen entries and
  their citations is an afternoon; it also touches another session's work mid-flight, and
  it does nothing to stop the sixteenth. The guard costs four lines and stops the class.

