# The model gets a door of its own


`./orbistoun.sh suggest [rounds]` asks a local model for candidate words and keeps what the
hash confirms. It is a **separate binary**, so `orbistoun-cli` gains no dependency on
`orbistoun-llm` - the CLI is what `run` calls, and that command has to stay fast. The run
report names the tool in its advice on an unnamed import and never invokes it (D265).

The shared loading and the loop moved into `orbistoun-propose::suggest`, so the binary and
the opt-in test share one copy rather than two.

**Its first real run banked nothing, and said so plainly**: *"nothing new. The words it
proposed were ones the grammar already holds."* That is the correct outcome - the shape
measurement says the current gap is shapes, not words - and a tool that reported it as
progress would be the failure this project spends most of its effort avoiding.

**And it exposed a cost nobody had looked at** (D266). One round costs 215 million
candidates when growing `learned`, 4.2 billion for `verb`, and 5.1 billion for `tail`,
because a round re-sweeps every pattern using the position it grew and `tail` is in nearly
all of them. The slot list asks the shortest list first, which is right about coverage and
twenty-three times wrong about cost.

### And the first run put a model runtime in the repository

`orbistoun-suggest` resolved its own data root and fell back to `.orbistoun` in the working
directory, which from here is the repository. The provenance guard failed on four downloaded
DLLs (D267).

Fixed by using `Paths::resolve`, which every other entry point already calls and which knows
about portable mode and the data-directory setting. The four lines that replaced it looked
too small to justify a dependency, which is how a second definition of a decision usually
gets written.

Worth noting what caught it: a guard written for firmware and disassembly, catching a build
artefact. Nobody had that case in mind when it was added.

