# orbistoun-audio

Audio output - the guest's audio-output library, reimplemented.

**Models:** declarations for init, open, close, output, and volume.

**Deliberately fakes:** all of it.

**Design note.** Audio is the subsystem most often stubbed to silence and left
there, which is worth naming as a trap: guests frequently **block on audio-buffer
completion**, so a stub that never signals a drained buffer hangs the title - with
no audio symptom to point at it. Silence is a safe output; never signalling is not.

**Status:** declarations only, arities provisional. Unscheduled - not reachable until
threading works, and writing it earlier means code that cannot be exercised.
