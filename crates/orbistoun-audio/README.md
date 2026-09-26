# orbistoun-audio

Audio output: the guest's audio-output library, reimplemented.

The crate serves the output ports - init, open, close, output, port state and volume - and
declares the rest of the audio libraries so a trace can name them. Its `implementations()`
list is wired into the registry by `modules()` in `crates/orbistoun-service/src/symbols.rs`.

## Buffer completion

Guests block on audio-buffer completion, so a port that never signals a drained buffer hangs
the title with no audio symptom to point at. **Rule:** output always drains. Silence is a
safe output; never signalling is not. The drain is modelled from the port's sample rate, so
output blocks for as long as the samples take, without depending on a host device.
